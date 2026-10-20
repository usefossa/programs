import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { createHash } from 'node:crypto';
import { Connection, Keypair, PublicKey } from '@solana/web3.js';
import { createClient } from 'redis';
import nacl from 'tweetnacl';

// default is read-only; execution requires an explicit flag and sufficient funding.
const root=resolve(import.meta.dir,'..');
const configPath=process.env.FOSSA_CONFIG;
if(!configPath)throw Error('Set FOSSA_CONFIG to an explicit operator configuration path.');
const config=Bun.TOML.parse(readFileSync(resolve(configPath),'utf8')) as {solana:{rpc_url:string};redis:{url:string;namespace:string};security:{encryption_key:string};marketplace:{program_id:string;market:string};provider:{wallet:string}};
const path=resolve(process.env.FOSSA_SBF??resolve(root,'target/deploy/inference_router_program.so'));
const artifact=readFileSync(path), hash=createHash('sha256').update(artifact).digest('hex');
const program=new PublicKey(config.marketplace.program_id),authority=new PublicKey(config.provider.wallet);
const rpc=new Connection(config.solana.rpc_url,'confirmed');
const executable=await rpc.getAccountInfo(program);
if(!executable?.executable || executable.data.readUInt32LE()!==2)throw Error('Expected existing upgradeable program');
const dataAddress=new PublicKey(executable.data.subarray(4,36));
const data=await rpc.getAccountInfo(dataAddress);
if(!data || data.data.readUInt32LE()!==3 || data.data[12]!==1 || !new PublicKey(data.data.subarray(13,45)).equals(authority))throw Error('Upgrade authority does not match');
if(data.data.subarray(45,45+artifact.length).equals(artifact)){
 const finalized=await rpc.getAccountInfo(dataAddress,'finalized');
 if(finalized?.data.subarray(45,45+artifact.length).equals(artifact)){console.log(JSON.stringify({program:program.toBase58(),alreadyDeployed:true,finalized:true,artifactSha256:hash}));process.exit(0)}
 throw Error('Matching upgrade is awaiting finality; do not submit another upgrade');
}
const growth=Math.max(0,artifact.length+45-data.data.length);
const extensionBytes=growth?Math.max(growth,10_240):0;
const extension=Math.max(0,await rpc.getMinimumBalanceForRentExemption(data.data.length+extensionBytes)-data.lamports);
const bufferRent=await rpc.getMinimumBalanceForRentExemption(artifact.length+37);
const balance=await rpc.getBalance(authority);
const required=extension+bufferRent+2_000_000;
console.log(JSON.stringify({program:program.toBase58(),authority:authority.toBase58(),artifactSha256:hash,bytes:artifact.length,extensionBytes,extensionLamports:extension,temporaryBufferLamports:bufferRent,feeReserveLamports:2000000,balanceLamports:balance,requiredLamports:required,shortfallLamports:Math.max(0,required-balance)}));
if(!process.argv.includes('--execute'))process.exit(0);
if(balance<required)throw Error('Upgrade funding insufficient; no transaction sent');
const redis=createClient({url:config.redis.url});await redis.connect();
const encryptionKey=Buffer.from(config.security.encryption_key,'hex');
const providerName=`${config.redis.namespace}:market:${program}:${config.marketplace.market}:provider:${authority}:key`;
const bufferName=`${config.redis.namespace}:upgrade:${program}:${hash}:buffer`;
let secret:Uint8Array|undefined,bufferSecret:Uint8Array|undefined;
try{
 const decrypt=(raw:string)=>{const value=JSON.parse(raw);const bytes=nacl.secretbox.open(Buffer.from(value.ciphertext,'hex'),Buffer.from(value.nonce,'hex'),encryptionKey);if(!bytes)throw Error('Signer decryption failed');return bytes};
 const stored=await redis.get(providerName);if(!stored)throw Error('Upgrade signer unavailable');secret=decrypt(stored);
 if(!Keypair.fromSecretKey(secret).publicKey.equals(authority))throw Error('Signer identity mismatch');
 if(!await redis.exists(bufferName)){
  const generated=Keypair.generate(),nonce=nacl.randomBytes(24);
  const ciphertext=nacl.secretbox(generated.secretKey,nonce,encryptionKey);
  await redis.set(bufferName,JSON.stringify({address:generated.publicKey.toBase58(),nonce:Buffer.from(nonce).toString('hex'),ciphertext:Buffer.from(ciphertext).toString('hex')}),{NX:true});
  generated.secretKey.fill(0);
 }
 bufferSecret=decrypt((await redis.get(bufferName))!);
 // linux memfd passes signer files to the cli without writing plaintext keys to disk.
 const script=`import os,sys,json,subprocess\nv=json.load(sys.stdin)\nfds=[]\nfor name in ('authority','buffer'):\n fd=os.memfd_create('fossa-'+name,0);os.write(fd,json.dumps(v[name]).encode());os.lseek(fd,0,0);fds.append(fd)\ncommon=['--keypair','/proc/self/fd/'+str(fds[0]),'--url',v['rpc'],'--output','json']\nif v['extensionBytes']:\n p=subprocess.run(['solana','program','extend',v['program'],str(v['extensionBytes']),'--commitment','finalized']+common,pass_fds=tuple(fds),capture_output=True,text=True)\n print((p.stdout+p.stderr).replace(v['rpc'],'[configured rpc]'),flush=True)\n if p.returncode:sys.exit(p.returncode)\nargs=['solana','program','deploy',v['artifact'],'--program-id',v['program'],'--upgrade-authority','/proc/self/fd/'+str(fds[0]),'--buffer','/proc/self/fd/'+str(fds[1]),'--use-rpc','--commitment','confirmed']+common\np=subprocess.run(args,pass_fds=tuple(fds),capture_output=True,text=True)\nprint((p.stdout+p.stderr).replace(v['rpc'],'[configured rpc]'))\nfor fd in fds:os.close(fd)\nsys.exit(p.returncode)\n`;
 const child=Bun.spawn(['python3','-c',script],{stdin:'pipe',stdout:'pipe',stderr:'pipe'});
 child.stdin.write(JSON.stringify({authority:[...secret],buffer:[...bufferSecret],artifact:path,program:program.toBase58(),rpc:config.solana.rpc_url,extensionBytes}));child.stdin.end();
 const [code,stdout,stderr]=await Promise.all([child.exited,new Response(child.stdout).text(),new Response(child.stderr).text()]);
 console.log(stdout.replaceAll(config.solana.rpc_url,'[configured rpc]'));if(stderr)console.error(stderr.replaceAll(config.solana.rpc_url,'[configured rpc]'));
 if(code!==0)throw Error('Upgrade incomplete; encrypted buffer retained for resume');
 let verified=false;
 for(let attempt=0;attempt<12;attempt++){
  const upgraded=await rpc.getAccountInfo(dataAddress,'finalized');
  if(upgraded?.data.subarray(45,45+artifact.length).equals(artifact)){verified=true;break}
  await new Promise(resolve=>setTimeout(resolve,5000));
 }
 if(!verified)throw Error('Program bytes not yet finalized; reconcile before another upgrade');
 await redis.set(`${config.redis.namespace}:upgrade:${program}:deployed`,JSON.stringify({sha256:hash,at:new Date().toISOString(),buffer:Keypair.fromSecretKey(bufferSecret).publicKey.toBase58()}));
 console.log('Finalized program bytes match artifact.');
}finally{secret?.fill(0);bufferSecret?.fill(0);encryptionKey.fill(0);await redis.quit()}

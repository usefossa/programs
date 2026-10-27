use crate::{
    abi::{FEE_MARKET_LEN, LIST_FEE_LEN, LIST_HEADER, LIST_MAGIC, MAX_ENTRIES},
    helpers::{add, err, rd16, rd32, rd64, require, wr32},
    state::{fresh, market, writable},
};
use pinocchio::{account::Ref, error::ProgramError, AccountView, Address, ProgramResult};

pub(crate) fn position(providers: &[u8], provider: &[u8]) -> Result<usize, usize> {
    let (mut low, mut high) = (0, providers.len() / 32);
    while low < high {
        let mid = low + (high - low) / 2;
        match providers[mid * 32..mid * 32 + 32].cmp(provider) {
            core::cmp::Ordering::Less => low = mid + 1,
            core::cmp::Ordering::Greater => high = mid,
            core::cmp::Ordering::Equal => return Ok(mid),
        }
    }
    Err(low)
}

pub(crate) struct Members<'a> {
    tree: &'a [[u64; 4]],
    prefix: usize,
}
pub(crate) fn rebuild(d: &mut [u8]) {
    let limit = rd32(d, 72) as usize;
    let count = rd32(d, 76) as usize;
    let (head, tail) = d.split_at_mut(80 + limit * 32);
    let tail = &mut tail[..limit * 32];
    tail.fill(0);
    // callers validate account alignment and bounded layout; cache contains integers only.
    let keys =
        unsafe { core::slice::from_raw_parts(head.as_ptr().add(80).cast::<[u64; 4]>(), count) };
    let tree =
        unsafe { core::slice::from_raw_parts_mut(tail.as_mut_ptr().cast::<[u64; 4]>(), limit) };
    let (mut node, mut at, mut depth) = (0, 0, 0);
    let mut stack = [0usize; 13];
    while node < count || depth > 0 {
        while node < count {
            stack[depth] = node;
            depth += 1;
            node = node * 2 + 1;
        }
        depth -= 1;
        node = stack[depth];
        tree[node] = keys[at].map(u64::to_be);
        at += 1;
        node = node * 2 + 2;
    }
}
impl<'a> Members<'a> {
    pub(crate) fn new(d: &'a [u8]) -> Self {
        let limit = rd32(d, 72) as usize;
        let count = rd32(d, 76) as usize;
        let mut prefix = 0;
        if count > 0 {
            let first = unsafe { &*d.as_ptr().add(80).cast::<[u64; 4]>() };
            let last = unsafe { &*d.as_ptr().add(80 + (count - 1) * 32).cast::<[u64; 4]>() };
            while prefix < 3 && first[prefix] == last[prefix] {
                prefix += 1;
            }
        }
        Self {
            prefix,
            tree: unsafe {
                core::slice::from_raw_parts(
                    d.as_ptr().add(80 + limit * 32).cast::<[u64; 4]>(),
                    count,
                )
            },
        }
    }
    #[inline(never)]
    pub(crate) fn contains(&self, provider: &[u8]) -> bool {
        // provider entries and the request word buffer are eight-byte aligned.
        let needle = unsafe { &*provider.as_ptr().cast::<[u64; 4]>() }.map(u64::to_be);
        if self.tree.is_empty() {
            return false;
        }
        for (i, value) in needle.iter().enumerate().take(self.prefix) {
            if self.tree[0][i] != *value {
                return false;
            }
        }
        match self.prefix {
            0 => self.find::<0>(&needle),
            1 => self.find::<1>(&needle),
            2 => self.find::<2>(&needle),
            _ => self.find::<3>(&needle),
        }
    }
    #[inline(always)]
    fn find<const START: usize>(&self, needle: &[u64; 4]) -> bool {
        let mut node = 0;
        let mut end = self.tree.len() * 32;
        // a full 4096-member tree has one thirteenth-level leaf: its smallest key.
        // resolve that key up front so every remaining lookup has at most twelve probes.
        if self.tree.len() == MAX_ENTRIES {
            let first = &self.tree[MAX_ENTRIES - 1];
            'above: {
                if START == 0 && first[0] != needle[0] {
                    if needle[0] < first[0] {
                        return false;
                    }
                    break 'above;
                }
                if START <= 1 && first[1] != needle[1] {
                    if needle[1] < first[1] {
                        return false;
                    }
                    break 'above;
                }
                if START <= 2 && first[2] != needle[2] {
                    if needle[2] < first[2] {
                        return false;
                    }
                    break 'above;
                }
                if needle[3] <= first[3] {
                    return needle[3] == first[3];
                }
            }
            end -= 32;
        }
        while node < end {
            let key = unsafe { &*self.tree.as_ptr().cast::<u8>().add(node).cast::<[u64; 4]>() };
            if START == 0 && key[0] != needle[0] {
                if key[0] < needle[0] {
                    node = node * 2 + 64;
                } else {
                    node = node * 2 + 32;
                }
                continue;
            }
            if START <= 1 && key[1] != needle[1] {
                if key[1] < needle[1] {
                    node = node * 2 + 64;
                } else {
                    node = node * 2 + 32;
                }
                continue;
            }
            if START <= 2 && key[2] != needle[2] {
                if key[2] < needle[2] {
                    node = node * 2 + 64;
                } else {
                    node = node * 2 + 32;
                }
                continue;
            }
            if key[3] != needle[3] {
                if key[3] < needle[3] {
                    node = node * 2 + 64;
                } else {
                    node = node * 2 + 32;
                }
                continue;
            }
            return true;
        }
        false
    }
}

pub(crate) fn state<'a>(
    a: &'a AccountView,
    program: &Address,
) -> Result<Ref<'a, [u8]>, ProgramError> {
    require(a.owner() == program && a.data_len() >= LIST_HEADER, 2)?;
    let d = a.try_borrow()?;
    let limit = rd32(&d, 72) as usize;
    require(
        d.as_ptr() as usize & 7 == 0
            && &d[..8] == LIST_MAGIC
            && (1..=MAX_ENTRIES).contains(&limit)
            && (d.len() == LIST_HEADER + limit * 64
                || d.len() == LIST_HEADER + limit * 64 + LIST_FEE_LEN)
            && rd32(&d, 76) as usize <= limit,
        2,
    )?;
    Ok(d)
}

pub(crate) fn initialize(
    program: &Address,
    accounts: &mut [AccountView],
    p: &[u8],
) -> ProgramResult {
    require(accounts.len() >= 3, 3)?;
    let (core, payment) = accounts.split_at_mut(3);
    let [m, list, authority] = core else {
        return Err(err(3));
    };
    require(authority.is_signer(), 4)?;
    let md = market(m, program)?;
    let fees = md.len() == FEE_MARKET_LEN;
    require(p.len() == if fees { 16 } else { 4 }, 3)?;
    if fees {
        super::fees::validate_terms(rd16(p, 4), rd16(p, 6))?;
        super::fees::creation_payment(&md, authority, payment, rd64(p, 8))?;
    } else {
        require(payment.is_empty(), 3)?;
    }
    let limit = rd32(p, 0) as usize;
    require((1..=MAX_ENTRIES).contains(&limit), 3)?;
    fresh(
        list,
        program,
        LIST_HEADER + limit * 64 + if fees { LIST_FEE_LEN } else { 0 },
    )?;
    let mut d = list.try_borrow_mut()?;
    d[..8].copy_from_slice(LIST_MAGIC);
    d[8..40].copy_from_slice(m.address().as_ref());
    d[40..72].copy_from_slice(authority.address().as_ref());
    wr32(&mut d, 72, limit as u32);
    if fees {
        let at = LIST_HEADER + limit * 64;
        d[at..at + 4].copy_from_slice(&p[4..8]);
    }
    Ok(())
}

pub(crate) fn update(program: &Address, accounts: &mut [AccountView], p: &[u8]) -> ProgramResult {
    let [m, list, authority] = accounts else {
        return Err(err(3));
    };
    require(p.len() == 33 && p[32] <= 1 && p[..32] != [0; 32], 3)?;
    writable(&[list])?;
    market(m, program)?;
    let d = state(list, program)?;
    require(&d[8..40] == m.address().as_ref(), 2)?;
    require(
        authority.is_signer() && &d[40..72] == authority.address().as_ref(),
        4,
    )?;
    let count = rd32(&d, 76) as usize;
    let found = position(&d[LIST_HEADER..LIST_HEADER + count * 32], &p[..32]);
    let (at, next) = if p[32] == 1 {
        require(count < rd32(&d, 72) as usize, 6)?;
        (found.err().ok_or(err(7))?, count + 1)
    } else {
        (found.map_err(|_| err(7))?, count - 1)
    };
    drop(d);
    let mut d = list.try_borrow_mut()?;
    let start = LIST_HEADER + at * 32;
    if p[32] == 1 {
        d.copy_within(start..LIST_HEADER + count * 32, start + 32);
        d[start..start + 32].copy_from_slice(&p[..32]);
    } else {
        d.copy_within(start + 32..LIST_HEADER + count * 32, start);
        d[LIST_HEADER + next * 32..LIST_HEADER + count * 32].fill(0);
    }
    wr32(&mut d, 76, next as u32);
    rebuild(&mut d);
    Ok(())
}

pub(crate) fn close(program: &Address, accounts: &mut [AccountView], p: &[u8]) -> ProgramResult {
    let [list, authority, metadata] = accounts else {
        return Err(err(3));
    };
    require(p.is_empty() && list.address() != authority.address(), 3)?;
    writable(&[list, authority, metadata])?;
    let d = state(list, program)?;
    require(
        authority.is_signer() && &d[40..72] == authority.address().as_ref(),
        4,
    )?;
    require(rd32(&d, 76) == 0, 7)?;
    let bump = super::list_metadata::canonical(metadata, list, program)?;
    let has_metadata = metadata.owner() != &pinocchio_system::ID || metadata.data_len() != 0;
    if has_metadata {
        super::list_metadata::validate(metadata, list, &d, program, bump)?;
    }
    drop(d);
    let refund = add(
        list.lamports(),
        if has_metadata { metadata.lamports() } else { 0 },
    )?;
    authority.set_lamports(add(authority.lamports(), refund)?);
    if has_metadata {
        metadata.close()?;
    }
    list.close()
}

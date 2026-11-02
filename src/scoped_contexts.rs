use crate::state_types::*;
use anchor_lang::{accounts::Slab, prelude::*};

#[derive(Accounts)]
pub struct ActivateListRouting {
    #[account(mut, constraint = market.council == *council.address() @ crate::RouterError::Unauthorized)]
    pub market: Account<Market>,
    pub council: Signer,
}

#[derive(Accounts)]
pub struct InitializeProviderPolicy {
    pub market: Account<Market>,
    pub provider: Signer,
    #[account(mut, unsafe(dup))]
    pub payer: Signer,
    #[account(constraint = profile.market == *market.address() && profile.authority == *provider.address() @ crate::RouterError::Unauthorized)]
    pub profile: Account<Provider>,
    #[account(init, payer = payer, seeds = [b"provider_policy", market.address().as_ref(), provider.address().as_ref()], bump)]
    pub policy: Account<ProviderPolicy>,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct ConfigureProviderPolicy {
    #[account(mut, constraint = policy.provider == *provider.address() @ crate::RouterError::Unauthorized)]
    pub policy: Account<ProviderPolicy>,
    pub provider: Signer,
}

#[derive(Accounts)]
#[instruction(_limit: u32)]
pub struct InitializeListRouting {
    pub market: Account<Market>,
    #[account(mut, constraint = list.market == *market.address() && list.authority == *authority.address() @ crate::RouterError::Unauthorized)]
    pub list: Slab<List, Address>,
    pub authority: Signer,
    #[account(mut, unsafe(dup))]
    pub payer: Signer,
    #[account(init, payer = payer, seeds = [b"list_routing", list.address().as_ref()], bump, space = Slab::<ListRouting, PolicyMirror>::space_for(_limit))]
    pub routing: Slab<ListRouting, PolicyMirror>,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct GrowListRouting {
    #[account(constraint = list.reserved[0] == 1 && list.authority == *authority.address() @ crate::RouterError::Unauthorized)]
    pub list: Slab<List, Address>,
    pub authority: Signer,
    #[account(mut, constraint = routing.list == *list.address() && routing.market == list.market @ crate::RouterError::InvalidAccount)]
    pub routing: Slab<ListRouting, PolicyMirror>,
    #[account(mut, unsafe(dup))]
    pub payer: Signer,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct UpdateRoutingList {
    #[account(mut, constraint = list.authority == *authority.address() @ crate::RouterError::Unauthorized)]
    pub list: Slab<List, Address>,
    pub authority: Signer,
    #[account(mut, constraint = routing.list == *list.address() && routing.market == list.market @ crate::RouterError::InvalidAccount)]
    pub routing: Slab<ListRouting, PolicyMirror>,
    #[account(mut, constraint = policy.market == list.market @ crate::RouterError::InvalidAccount)]
    pub policy: Option<Account<ProviderPolicy>>,
}

#[derive(Accounts)]
pub struct EnablePolicyMirror {
    pub provider: Signer,
    #[account(mut, constraint = policy.provider == *provider.address() @ crate::RouterError::Unauthorized)]
    pub policy: Account<ProviderPolicy>,
    #[account(constraint = list.market == policy.market && list.reserved[0] == 1 @ crate::RouterError::InvalidAccount)]
    pub list: Slab<List, Address>,
    #[account(mut, constraint = routing.list == *list.address() && routing.market == policy.market @ crate::RouterError::InvalidAccount)]
    pub routing: Slab<ListRouting, PolicyMirror>,
}

#[derive(Accounts)]
pub struct FreezePolicyMirror {
    pub signer: Signer,
    #[account(mut, constraint = policy.market == list.market @ crate::RouterError::InvalidAccount)]
    pub policy: Account<ProviderPolicy>,
    pub list: Slab<List, Address>,
    #[account(mut, constraint = routing.list == *list.address() && routing.market == policy.market @ crate::RouterError::InvalidAccount)]
    pub routing: Slab<ListRouting, PolicyMirror>,
}

#[derive(Accounts)]
pub struct CloseListRouting {
    #[account(mut, constraint = list.authority == *authority.address() @ crate::RouterError::Unauthorized)]
    pub list: Slab<List, Address>,
    #[account(mut)]
    pub authority: Signer,
    #[account(mut, constraint = routing.list == *list.address() && routing.market == list.market @ crate::RouterError::InvalidAccount)]
    pub routing: Slab<ListRouting, PolicyMirror>,
}

#[derive(Accounts)]
pub struct InitializeModelCapacity {
    #[account(constraint = market.reserved[0] == 1 @ crate::RouterError::InvalidState)]
    pub market: Account<Market>,
    #[account(constraint = model.market == *market.address() @ crate::RouterError::InvalidAccount)]
    pub model: Account<Model>,
    pub provider: Signer,
    #[account(mut, unsafe(dup))]
    pub payer: Signer,
    #[account(constraint = profile.market == *market.address() && profile.authority == *provider.address() @ crate::RouterError::Unauthorized)]
    pub profile: Account<Provider>,
    #[account(address = model.terms @ crate::RouterError::InvalidAccount)]
    pub legacy_terms: UncheckedAccount,
    #[account(address = model.capacity @ crate::RouterError::InvalidAccount, constraint = legacy_capacity.model == *model.address() @ crate::RouterError::InvalidAccount)]
    pub legacy_capacity: Slab<Capacity, CapacityEntry>,
    #[account(init, payer = payer, seeds = [b"provider_capacity", model.address().as_ref(), provider.address().as_ref()], bump)]
    pub budget: Account<ProviderModelCapacity>,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct ConfigureModelCapacity {
    pub provider: Signer,
    #[account(mut, constraint = budget.provider == *provider.address() @ crate::RouterError::Unauthorized)]
    pub budget: Account<ProviderModelCapacity>,
    #[account(address = budget.model @ crate::RouterError::InvalidAccount)]
    pub model: Account<Model>,
    #[account(address = model.capacity @ crate::RouterError::InvalidAccount, constraint = legacy_capacity.model == *model.address() @ crate::RouterError::InvalidAccount)]
    pub legacy_capacity: Slab<Capacity, CapacityEntry>,
}

#[derive(Accounts)]
pub struct SyncLegacyCapacity {
    #[account(mut)]
    pub budget: Account<ProviderModelCapacity>,
    #[account(address = budget.model @ crate::RouterError::InvalidAccount)]
    pub model: Account<Model>,
    #[account(address = model.capacity @ crate::RouterError::InvalidAccount, constraint = legacy_capacity.model == *model.address() @ crate::RouterError::InvalidAccount)]
    pub legacy_capacity: Slab<Capacity, CapacityEntry>,
}

#[derive(Accounts)]
#[instruction(_limit: u32)]
pub struct InitializeListBooks {
    pub market: Account<Market>,
    #[account(constraint = model.market == *market.address() @ crate::RouterError::InvalidAccount)]
    pub model: Account<Model>,
    #[account(constraint = list.market == *market.address() && list.reserved[0] == 1 && list.authority == *authority.address() @ crate::RouterError::Unauthorized)]
    pub list: Slab<List, Address>,
    pub authority: Signer,
    #[account(mut, constraint = routing.list == *list.address() && routing.market == *market.address() @ crate::RouterError::InvalidAccount)]
    pub routing: Slab<ListRouting, PolicyMirror>,
    #[account(mut, unsafe(dup))]
    pub payer: Signer,
    #[account(init, payer = payer, seeds = [b"list_terms", list.address().as_ref(), model.address().as_ref()], bump, space = Slab::<ListTerms, ListOffer>::space_for(_limit))]
    pub terms: Slab<ListTerms, ListOffer>,
    #[account(init, payer = payer, seeds = [b"list_capacity", list.address().as_ref(), model.address().as_ref()], bump, space = Slab::<ListCapacity, ListCapacityEntry>::space_for(_limit))]
    pub capacity: Slab<ListCapacity, ListCapacityEntry>,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct GrowListBooks {
    #[account(constraint = list.authority == *authority.address() @ crate::RouterError::Unauthorized)]
    pub list: Slab<List, Address>,
    pub authority: Signer,
    #[account(mut, constraint = terms.list == *list.address() @ crate::RouterError::InvalidAccount)]
    pub terms: Slab<ListTerms, ListOffer>,
    #[account(mut, address = terms.capacity @ crate::RouterError::InvalidAccount, constraint = capacity.terms == *terms.address() @ crate::RouterError::InvalidAccount)]
    pub capacity: Slab<ListCapacity, ListCapacityEntry>,
    #[account(mut, unsafe(dup))]
    pub payer: Signer,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct CloseListBooks {
    #[account(constraint = list.authority == *authority.address() @ crate::RouterError::Unauthorized)]
    pub list: Slab<List, Address>,
    #[account(mut)]
    pub authority: Signer,
    #[account(mut, constraint = routing.list == *list.address() @ crate::RouterError::InvalidAccount)]
    pub routing: Slab<ListRouting, PolicyMirror>,
    #[account(mut, constraint = terms.list == *list.address() @ crate::RouterError::InvalidAccount)]
    pub terms: Slab<ListTerms, ListOffer>,
    #[account(mut, address = terms.capacity @ crate::RouterError::InvalidAccount, constraint = capacity.terms == *terms.address() @ crate::RouterError::InvalidAccount)]
    pub capacity: Slab<ListCapacity, ListCapacityEntry>,
}

#[derive(Accounts)]
pub struct SetListOffer {
    pub market: Account<Market>,
    #[account(constraint = model.market == *market.address() @ crate::RouterError::InvalidAccount)]
    pub model: Account<Model>,
    pub provider: Signer,
    #[account(mut, constraint = budget.model == *model.address() && budget.provider == *provider.address() @ crate::RouterError::Unauthorized)]
    pub budget: Account<ProviderModelCapacity>,
    #[account(constraint = list.market == *market.address() @ crate::RouterError::InvalidAccount)]
    pub list: Slab<List, Address>,
    #[account(constraint = routing.list == *list.address() && routing.market == *market.address() @ crate::RouterError::InvalidAccount)]
    pub routing: Slab<ListRouting, PolicyMirror>,
    #[account(mut, constraint = terms.model == *model.address() && terms.list == *list.address() @ crate::RouterError::InvalidAccount)]
    pub terms: Slab<ListTerms, ListOffer>,
    #[account(mut, address = terms.capacity @ crate::RouterError::InvalidAccount, constraint = capacity.terms == *terms.address() @ crate::RouterError::InvalidAccount)]
    pub capacity: Slab<ListCapacity, ListCapacityEntry>,
    #[account(constraint = payout.mint() == &market.mint && payout.owner() == provider.address() @ crate::RouterError::InvalidAccount)]
    pub payout: Account<anchor_spl::token::TokenAccount>,
}

#[derive(Accounts)]
pub struct PruneListOffer {
    pub list: Slab<List, Address>,
    #[account(mut, constraint = terms.list == *list.address() @ crate::RouterError::InvalidAccount)]
    pub terms: Slab<ListTerms, ListOffer>,
    #[account(mut, address = terms.capacity @ crate::RouterError::InvalidAccount, constraint = capacity.terms == *terms.address() @ crate::RouterError::InvalidAccount)]
    pub capacity: Slab<ListCapacity, ListCapacityEntry>,
    #[account(mut, constraint = budget.model == terms.model @ crate::RouterError::InvalidAccount)]
    pub budget: Account<ProviderModelCapacity>,
}

#[derive(Accounts)]
#[instruction(_limit: u32)]
pub struct InitializeProviderWallet {
    pub market: Account<Market>,
    pub owner: Signer,
    #[account(mut, unsafe(dup))]
    pub payer: Signer,
    #[account(init, payer = payer, seeds = [b"provider_wallet", market.address().as_ref(), owner.address().as_ref()], bump, space = Slab::<ProviderWallet, ProviderUsage>::space_for(_limit))]
    pub wallet: Slab<ProviderWallet, ProviderUsage>,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct GrowProviderWallet {
    pub owner: Signer,
    #[account(mut, constraint = wallet.owner == *owner.address() @ crate::RouterError::Unauthorized)]
    pub wallet: Slab<ProviderWallet, ProviderUsage>,
    #[account(mut, unsafe(dup))]
    pub payer: Signer,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct PruneProviderWallet {
    pub owner: Signer,
    #[account(mut, constraint = wallet.owner == *owner.address() @ crate::RouterError::Unauthorized)]
    pub wallet: Slab<ProviderWallet, ProviderUsage>,
}

#[derive(Accounts)]
pub struct CloseProviderWallet {
    pub owner: Signer,
    #[account(mut, constraint = wallet.owner == *owner.address() @ crate::RouterError::Unauthorized)]
    pub wallet: Slab<ProviderWallet, ProviderUsage>,
    #[account(mut, unsafe(dup), address = wallet.rent_payer @ crate::RouterError::InvalidAccount)]
    pub recipient: UncheckedAccount,
}

#[derive(Accounts)]
pub struct ReserveListBooking {
    #[account(constraint = market.reserved[0] == 1 @ crate::RouterError::InvalidState)]
    pub market: Account<Market>,
    #[account(constraint = model.market == *market.address() @ crate::RouterError::InvalidAccount)]
    pub model: Account<Model>,
    #[account(constraint = terms.model == *model.address() && terms.list == *list.address() @ crate::RouterError::InvalidAccount)]
    pub terms: Slab<ListTerms, ListOffer>,
    #[account(mut, address = terms.capacity @ crate::RouterError::InvalidAccount, constraint = capacity.terms == *terms.address() && capacity.len() == terms.len() @ crate::RouterError::InvalidAccount)]
    pub capacity: Slab<ListCapacity, ListCapacityEntry>,
    #[account(mut, constraint = user.market == *market.address() && user.owner == *owner.address() @ crate::RouterError::Unauthorized)]
    pub user: Account<User>,
    pub owner: Signer,
    #[account(mut)]
    pub booking: Account<Booking>,
    pub rent_payer: UncheckedAccount,
    #[account(constraint = list.market == *market.address() && list.reserved[0] == 1 @ crate::RouterError::InvalidAccount)]
    pub list: Slab<List, Address>,
    #[account(constraint = routing.list == *list.address() && routing.market == *market.address() @ crate::RouterError::InvalidAccount)]
    pub routing: Slab<ListRouting, PolicyMirror>,
    #[account(mut, constraint = wallet.market == *market.address() && wallet.owner == *owner.address() @ crate::RouterError::InvalidAccount)]
    pub wallet: Slab<ProviderWallet, ProviderUsage>,
    #[account(address = model.group @ crate::RouterError::InvalidAccount, constraint = group.market == *market.address() @ crate::RouterError::InvalidAccount)]
    pub group: Option<Account<ModelGroup>>,
}

use cw_storage_plus::Item;
use white_whale_std::vault_network::vault_router::Config;

pub const CONFIG: Item<Config> = Item::new("config");

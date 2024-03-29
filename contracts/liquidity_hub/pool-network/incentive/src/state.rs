use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};
use white_whale_std::pool_network::incentive::{ClosedPosition, Config, Flow, OpenPosition};

/// The configuration for the incentive contract.
pub const CONFIG: Item<Config> = Item::new("config");

/// An monotonically increasing counter to generate unique flow identifiers.
pub const FLOW_COUNTER: Item<u64> = Item::new("flow_counter");

/// The current flows that exist. Key is (start_epoch_id, flow_id)
pub const FLOWS: Map<(EpochId, FlowId), Flow> = Map::new("flows");

/// All open positions that users have.
pub const OPEN_POSITIONS: Map<Addr, Vec<OpenPosition>> = Map::new("open_positions");
/// All closed positions that users have.
pub const CLOSED_POSITIONS: Map<Addr, Vec<ClosedPosition>> = Map::new("closed_positions");

/// The global weight (sum of all individual weights)
pub const GLOBAL_WEIGHT: Item<Uint128> = Item::new("global_weight");
/// The weights for individual accounts
pub const ADDRESS_WEIGHT: Map<Addr, Uint128> = Map::new("address_weight");

/// GLOBAL_WEIGHT_SNAPSHOT and ADDRESS_WEIGHT_HISTORY are used to calculate the deterministically
/// calculate the rewards for a given address at a given epoch.

/// The global weight snapshots, sum of all individual weights at a given epoch
pub const GLOBAL_WEIGHT_SNAPSHOT: Map<EpochId, Uint128> = Map::new("global_weight_snapshot");
/// The address weight history, i.e. how much weight an address had at a given epoch. Key is (address, epoch_id)
pub const ADDRESS_WEIGHT_HISTORY: Map<(&Addr, EpochId), Uint128> =
    Map::new("address_weight_snapshot");
/// The last epoch an address claimed rewards
pub const LAST_CLAIMED_EPOCH: Map<&Addr, EpochId> = Map::new("last_claimed_epoch");

pub type EpochId = u64;
pub type FlowId = u64;

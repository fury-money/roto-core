use cosmwasm_std::{
    Addr, BankMsg, Coin, CosmosMsg, Decimal, DepsMut, Env, MessageInfo, Order, Response, StdResult,
    Timestamp, Uint128, Uint64,
};
use white_whale_std::pool_network::asset;
use white_whale_std::pool_network::asset::{Asset, AssetInfo};

use white_whale_std::whale_lair::Bond;

use crate::helpers::validate_growth_rate;
use crate::queries::MAX_PAGE_LIMIT;
use crate::state::{update_global_weight, update_local_weight, BOND, CONFIG, GLOBAL, UNBOND};
use crate::{helpers, ContractError};

/// Bonds the provided asset.
pub(crate) fn bond(
    mut deps: DepsMut,
    timestamp: Timestamp,
    info: MessageInfo,
    env: Env,
    asset: Asset,
) -> Result<Response, ContractError> {
    // validate the denom sent is the whitelisted one for bonding
    let denom = match asset.info.clone() {
        AssetInfo::NativeToken { denom } => denom,
        AssetInfo::Token { .. } => return Err(ContractError::InvalidBondingAsset {}),
    };

    helpers::validate_funds(&deps, &info, &asset, denom.clone())?;
    helpers::validate_claimed(&deps, &info)?;
    helpers::validate_bonding_for_current_epoch(&deps, &env)?;

    let mut bond = BOND
        .key((&info.sender, &denom))
        .may_load(deps.storage)?
        .unwrap_or(Bond {
            asset: Asset {
                amount: Uint128::zero(),
                ..asset.clone()
            },
            ..Bond::default()
        });

    // update local values
    bond.asset.amount = bond.asset.amount.checked_add(asset.amount)?;
    // let new_bond_weight = get_weight(timestamp, bond.weight, asset.amount, config.growth_rate, bond.timestamp)?;
    bond.weight = bond.weight.checked_add(asset.amount)?;
    bond = update_local_weight(&mut deps, info.sender.clone(), timestamp, bond)?;

    BOND.save(deps.storage, (&info.sender, &denom), &bond)?;

    // update global values
    let mut global_index = GLOBAL.may_load(deps.storage)?.unwrap_or_default();
    // global_index = update_global_weight(&mut deps, timestamp, global_index)?;

    // include time term in the weight
    global_index.weight = global_index.weight.checked_add(asset.amount)?;
    global_index.bonded_amount = global_index.bonded_amount.checked_add(asset.amount)?;
    global_index.bonded_assets =
        asset::aggregate_assets(global_index.bonded_assets, vec![asset.clone()])?;
    global_index = update_global_weight(&mut deps, timestamp, global_index)?;

    GLOBAL.save(deps.storage, &global_index)?;

    Ok(Response::default().add_attributes(vec![
        ("action", "bond".to_string()),
        ("address", info.sender.to_string()),
        ("asset", asset.to_string()),
    ]))
}

/// Unbonds the provided amount of tokens
pub(crate) fn unbond(
    mut deps: DepsMut,
    timestamp: Timestamp,
    info: MessageInfo,
    env: Env,
    asset: Asset,
) -> Result<Response, ContractError> {
    if asset.amount.is_zero() {
        return Err(ContractError::InvalidUnbondingAmount {});
    }

    let denom = match asset.info.clone() {
        AssetInfo::NativeToken { denom } => denom,
        AssetInfo::Token { .. } => return Err(ContractError::InvalidBondingAsset {}),
    };

    helpers::validate_claimed(&deps, &info)?;
    helpers::validate_bonding_for_current_epoch(&deps, &env)?;

    if let Some(mut unbond) = BOND.key((&info.sender, &denom)).may_load(deps.storage)? {
        // check if the address has enough bond
        if unbond.asset.amount < asset.amount {
            return Err(ContractError::InsufficientBond {});
        }
        // update local values, decrease the bond
        unbond = update_local_weight(&mut deps, info.sender.clone(), timestamp, unbond.clone())?;
        let weight_slash = unbond.weight * Decimal::from_ratio(asset.amount, unbond.asset.amount);
        unbond.weight = unbond.weight.checked_sub(weight_slash)?;
        unbond.asset.amount = unbond.asset.amount.checked_sub(asset.amount)?;

        if unbond.asset.amount.is_zero() {
            BOND.remove(deps.storage, (&info.sender, &denom));
        } else {
            BOND.save(deps.storage, (&info.sender, &denom), &unbond)?;
        }

        // record the unbonding
        UNBOND.save(
            deps.storage,
            (&info.sender, &denom, timestamp.nanos()),
            &Bond {
                asset: asset.clone(),
                weight: Uint128::zero(),
                timestamp,
            },
        )?;

        // update global values
        let mut global_index = GLOBAL.may_load(deps.storage)?.unwrap_or_default();
        global_index = update_global_weight(&mut deps, timestamp, global_index)?;
        global_index.bonded_amount = global_index.bonded_amount.checked_sub(asset.amount)?;
        global_index.bonded_assets =
            asset::deduct_assets(global_index.bonded_assets, vec![asset.clone()])?;
        global_index.weight = global_index.weight.checked_sub(weight_slash)?;

        GLOBAL.save(deps.storage, &global_index)?;

        Ok(Response::default().add_attributes(vec![
            ("action", "unbond".to_string()),
            ("address", info.sender.to_string()),
            ("asset", asset.to_string()),
        ]))
    } else {
        Err(ContractError::NothingToUnbond {})
    }
}

/// Withdraws the rewards for the provided address
pub(crate) fn withdraw(
    deps: DepsMut,
    timestamp: Timestamp,
    address: Addr,
    denom: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let unbondings: Vec<(u64, Bond)> = UNBOND
        .prefix((&address, &denom))
        .range(deps.storage, None, None, Order::Ascending)
        .take(MAX_PAGE_LIMIT as usize)
        .collect::<StdResult<Vec<(u64, Bond)>>>()?;

    let mut refund_amount = Uint128::zero();

    if unbondings.is_empty() {
        return Err(ContractError::NothingToWithdraw {});
    }

    for unbonding in unbondings {
        let (ts, bond) = unbonding;
        if timestamp.minus_nanos(config.unbonding_period.u64()) >= bond.timestamp {
            let denom = match bond.asset.info {
                AssetInfo::Token { .. } => return Err(ContractError::InvalidBondingAsset {}),
                AssetInfo::NativeToken { denom } => denom,
            };

            refund_amount = refund_amount.checked_add(bond.asset.amount)?;
            UNBOND.remove(deps.storage, (&address, &denom, ts));
        }
    }

    let refund_msg = CosmosMsg::Bank(BankMsg::Send {
        to_address: address.to_string(),
        amount: vec![Coin {
            denom: denom.clone(),
            amount: refund_amount,
        }],
    });

    Ok(Response::default()
        .add_message(refund_msg)
        .add_attributes(vec![
            ("action", "withdraw".to_string()),
            ("address", address.to_string()),
            ("denom", denom),
            ("refund_amount", refund_amount.to_string()),
        ]))
}

/// Updates the configuration of the contract
pub(crate) fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    unbonding_period: Option<Uint64>,
    growth_rate: Option<Decimal>,
    fee_distributor_addr: Option<String>,
) -> Result<Response, ContractError> {
    // check the owner is the one who sent the message
    let mut config = CONFIG.load(deps.storage)?;
    if config.owner != info.sender {
        return Err(ContractError::Unauthorized {});
    }

    if let Some(owner) = owner {
        config.owner = deps.api.addr_validate(&owner)?;
    }

    if let Some(unbonding_period) = unbonding_period {
        config.unbonding_period = unbonding_period;
    }

    if let Some(growth_rate) = growth_rate {
        validate_growth_rate(growth_rate)?;
        config.growth_rate = growth_rate;
    }

    if let Some(fee_distributor_addr) = fee_distributor_addr {
        config.fee_distributor_addr = deps.api.addr_validate(&fee_distributor_addr)?;
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::default().add_attributes(vec![
        ("action", "update_config".to_string()),
        ("owner", config.owner.to_string()),
        ("unbonding_period", config.unbonding_period.to_string()),
        ("growth_rate", config.growth_rate.to_string()),
    ]))
}

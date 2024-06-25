use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;

use candid::{CandidType, Principal};
use ic_cdk::{api, post_upgrade, pre_upgrade, query, storage, update};
use serde::{Deserialize, Serialize};

const ANONYMOUS: Principal = Principal::anonymous();

type Collections = HashMap<usize, Collection>;
type Nfts = HashMap<u64, Nft>;
type Custodians = HashSet<Principal>;
type Operators = HashMap<Principal, HashSet<Principal>>;

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct State {
    collections: Collections,
    txid: u128,
}

impl State {
    pub fn next_txid(&mut self) -> u128 {
        self.txid += 1;
        self.txid
    }
}

#[derive(CandidType, Serialize, Deserialize, Clone, Default)]
pub enum LogoExtension {
    #[default]
    Png,
    Jpg,
    Jpeg,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Default)]
pub struct Logo {
    extension: LogoExtension,
    data: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Collection {
    name: String,
    logo: Logo,
    symbol: String,
    nfts: Nfts,
    custodians: Custodians,
    operators: Operators,
}

#[derive(CandidType, Serialize, Deserialize, Clone)]
pub struct Nft {
    id: u64,
    owner: Principal,
    approved: Option<Principal>,
    metadata: Vec<MetadataPart>,
    content: Vec<u8>,
}

#[derive(CandidType, Serialize, Deserialize, Clone)]
pub struct MetadataPart {
    purpose: MetadataPurpose,
    key_val_data: HashMap<String, MetadataValue>,
    data: Vec<u8>,
}

#[derive(CandidType, Serialize, Deserialize, PartialEq, Clone)]
pub enum MetadataPurpose {
    Preview,
    Rendered,
}

#[derive(CandidType, Serialize, Deserialize, Clone)]
enum MetadataValue {
    Text(String),
    Blob(Vec<u8>),
    Nat8(u8),
    Nat16(u16),
    Nat32(u32),
    Nat64(u64),
    Nat(u128),
}

#[derive(CandidType, Deserialize)]
enum Interface {
    Approval,
    Burn,
    TransferNotification,
}

#[pre_upgrade]
fn pre_upgrade() {
    let serialized_state = serde_cbor::to_vec(&STATE.with(|state| state.borrow().clone()))
        .expect("failed to serialize collections");
    storage::stable_save((serialized_state,)).expect("failed to stable save")
}
#[post_upgrade]
fn post_upgrade() {
    let (state,): (Vec<u8>,) = storage::stable_restore().expect("failed to restore");
    let deserialized_state: State =
        serde_cbor::from_slice(&state).expect("failed to deserialize collections");
    STATE.with(|state| {
        let mut borrowed = state.borrow_mut();
        borrowed.collections.extend(deserialized_state.collections);
        borrowed.txid = deserialized_state.txid
    })
}

#[derive(CandidType, Deserialize)]
pub struct InsertCollection {
    name: String,
    logo: Logo,
    symbol: String,
}

#[update]
pub fn insert_collection(collection: InsertCollection) -> usize {
    let id = STATE.with(|state| state.borrow().collections.len().wrapping_add(1));
    STATE.with(|state| {
        state.borrow_mut().collections.insert(
            id,
            Collection {
                name: collection.name,
                logo: collection.logo,
                symbol: collection.symbol,
                ..Default::default()
            },
        )
    });
    id
}

#[update]
fn set_name_of_collection(collection_id: usize, name: String) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let collection = state
            .collections
            .get_mut(&collection_id)
            .expect("invalid collection id");
        if collection.custodians.contains(&api::caller()) {
            collection.name = name
        } else {
            panic!("unauthorized")
        }
    })
}

#[update]
fn set_symbol_of_collection(collection_id: usize, symbol: String) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let collection = state
            .collections
            .get_mut(&collection_id)
            .expect("invalid collection id");
        if collection.custodians.contains(&api::caller()) {
            collection.symbol = symbol
        } else {
            panic!("unauthorized")
        }
    })
}

#[update]
fn set_logo_of_collection(collection_id: usize, logo: Logo) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let collection = state
            .collections
            .get_mut(&collection_id)
            .expect("invalid collection id");
        if collection.custodians.contains(&api::caller()) {
            collection.logo = logo
        } else {
            panic!("unauthorized")
        }
    })
}

#[query]
fn name_of_collection(collection_id: usize) -> Option<String> {
    STATE.with(|state| {
        state
            .borrow()
            .collections
            .get(&collection_id)
            .map(|collection| collection.name.to_owned())
    })
}

#[query]
fn symbol_of_collection(collection_id: usize) -> Option<String> {
    STATE.with(|state| {
        state
            .borrow()
            .collections
            .get(&collection_id)
            .map(|collection| collection.symbol.to_owned())
    })
}

#[query]
fn logo_of_collection(collection_id: usize) -> Option<Logo> {
    STATE.with(|state| {
        state
            .borrow()
            .collections
            .get(&collection_id)
            .map(|collection| collection.logo.to_owned())
    })
}

#[query]
fn balance_of_user(collection_id: usize, principal: Principal) -> usize {
    STATE.with(|state| {
        state
            .borrow()
            .collections
            .get(&collection_id)
            .map(|collection| {
                collection
                    .nfts
                    .values()
                    .filter(|nft| nft.owner == principal)
                    .count()
            })
            .unwrap_or_default()
    })
}

#[query]
fn owner_of_nft(collection_id: usize, token_id: u64) -> Option<Principal> {
    STATE.with(|state| {
        state
            .borrow()
            .collections
            .get(&collection_id)
            .and_then(|collection| {
                collection
                    .nfts
                    .values()
                    .find(|nft| nft.id == token_id)
                    .map(|nft| nft.owner)
            })
    })
}

#[update]
fn transfer_from_to(collection_id: usize, token_id: u64, from: Principal, to: Principal) -> u128 {
    if to == ANONYMOUS {
        panic!("zero address")
    } else {
        let caller = api::caller();
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let collection = state
                .collections
                .get_mut(&collection_id)
                .expect("invalid collection id");
            let nft = collection
                .nfts
                .get_mut(&token_id)
                .expect("invalid token id");
            if nft.owner != caller
                && nft.approved != Some(caller)
                && !collection
                    .operators
                    .get(&from)
                    .map(|operators| operators.contains(&caller))
                    .unwrap_or(false)
                && !collection.custodians.contains(&caller)
            {
                panic!("unauthorized")
            } else if nft.owner != from {
                panic!("other")
            } else {
                nft.approved = None;
                nft.owner = to;
                state.next_txid()
            }
        })
    }
}

#[query]
fn supported_interfaces() -> Vec<Interface> {
    vec![
        Interface::Approval,
        Interface::TransferNotification,
        Interface::Burn,
    ]
}

#[query]
fn total_supply() -> usize {
    STATE.with(|state| {
        let state = state.borrow();
        let mut total = 0;
        for collection in state.collections.values() {
            total += collection.nfts.len()
        }
        total
    })
}

#[query]
fn total_supply_of_collection(collection_id: usize) -> Option<usize> {
    STATE.with(|state| {
        state
            .borrow()
            .collections
            .get(&collection_id)
            .map(|collection| collection.nfts.len())
    })
}

#[update]
fn insert_custodian_into_collection(collection_id: usize, custodian: Principal) -> bool {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let collection = state
            .collections
            .get_mut(&collection_id)
            .expect("invalid collection id");
        if collection.custodians.contains(&api::caller()) {
            collection.custodians.insert(custodian)
        } else {
            panic!("unauthorized")
        }
    })
}

#[update]
fn remove_custodian_from_collection(collection_id: usize, custodian: Principal) -> bool {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let collection = state
            .collections
            .get_mut(&collection_id)
            .expect("invalid collection id");
        if collection.custodians.contains(&api::caller()) {
            collection.custodians.remove(&custodian)
        } else {
            panic!("unauthorized")
        }
    })
}

#[query]
fn is_custodian_of_collection(collection_id: usize, custodian: Principal) -> bool {
    STATE.with(|state| {
        let state = state.borrow();
        let collection = state
            .collections
            .get(&collection_id)
            .expect("invalid collection id");
        collection.custodians.contains(&custodian)
    })
}

#[update]
fn approve(collection_id: usize, token_id: u64, user: Principal) -> u128 {
    let caller = api::caller();
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let collection = state
            .collections
            .get_mut(&collection_id)
            .expect("invalid collection id");
        let nft = collection
            .nfts
            .get_mut(&token_id)
            .expect("invalid token id");
        if nft.owner != caller
            && nft.approved != Some(caller)
            && !collection
                .operators
                .get(&user)
                .map(|operators| operators.contains(&caller))
                .unwrap_or(false)
            && !collection.custodians.contains(&caller)
        {
            panic!("unauthorized")
        } else {
            nft.approved = Some(user);
            state.next_txid()
        }
    })
}

#[update]
fn set_approval_for_all(collection_id: usize, operator: Principal, is_approved: bool) -> u128 {
    let caller = api::caller();
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let collection = state
            .collections
            .get_mut(&collection_id)
            .expect("invalid collection id");
        if operator != caller {
            let operators = collection.operators.entry(caller).or_default();
            if operator == ANONYMOUS {
                if !is_approved {
                    operators.clear()
                } else {
                    // cannot enable everyone as an operator
                }
            } else if is_approved {
                operators.insert(operator);
            } else {
                operators.remove(&operator);
            }
        }
        state.next_txid()
    })
}

#[query]
fn is_approved_for_all(collection_id: usize, operator: Principal) -> bool {
    STATE.with(|state| {
        let state = state.borrow();
        let collection = state
            .collections
            .get(&collection_id)
            .expect("invalid collection id");
        collection
            .operators
            .get(&api::caller())
            .map(|s| s.contains(&operator))
            .unwrap_or(false)
    })
}

#[update]
fn burn(collection_id: usize, token_id: u64) -> u128 {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let collection = state
            .collections
            .get_mut(&collection_id)
            .expect("invalid token id");
        let nft = collection
            .nfts
            .get_mut(&token_id)
            .expect("invalid token id");
        if nft.owner != api::caller() {
            panic!("unauthorized")
        } else {
            nft.owner = ANONYMOUS;
            state.next_txid()
        }
    })
}

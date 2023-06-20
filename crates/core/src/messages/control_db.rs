use spacetimedb_lib::{Hash, Identity};
use spacetimedb_sats::de::Deserialize;
use spacetimedb_sats::ser::Serialize;

use crate::address::Address;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct IdentityEmail {
    pub identity: Identity,
    pub email: String,
}
/// An energy balance (per identity).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct EnergyBalance {
    pub identity: Identity,
    /// How much budget is remaining for this identity.
    pub balance_quanta: i64,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Database {
    pub id: u64,
    pub address: Address,
    pub identity: Identity,
    pub host_type: HostType,
    pub num_replicas: u32,
    pub program_bytes_address: Hash,
    /// Whether to create a full event log of all database events, for diagnostic / replay purposes.
    pub trace_log: bool,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DatabaseStatus {
    pub state: String,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DatabaseInstance {
    pub id: u64,
    pub database_id: u64,
    pub node_id: u64,
    pub leader: bool,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DatabaseInstanceStatus {
    pub state: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: u64,
    pub unschedulable: bool,
    /// TODO: It's unclear if this should be in here since it's arguably status
    /// rather than part of the configuration kind of. I dunno.
    pub advertise_addr: String,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeStatus {
    /// TODO: node memory, CPU, and storage capacity
    /// TODO: node memory, CPU, and storage allocatable capacity
    /// SEE: <https://kubernetes.io/docs/reference/kubernetes-api/cluster-resources/node-v1/#NodeStatus>
    pub state: String,
}
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, strum::EnumString, strum::AsRefStr,
)]
#[strum(serialize_all = "lowercase")]
#[repr(i32)]
pub enum HostType {
    Wasmer = 0,
}
#![no_std]

use soroban_sdk::{Address, Env, IntoVal, Val};

/// Trait to identify the Admin key for a contract's DataKey enum
pub trait AdminKey {
    fn admin_key() -> Self;
}

pub fn require_admin<K>(env: &Env) -> Option<Address>
where
    K: AdminKey + IntoVal<Env, Val>,
{
    env.storage().instance().get(&K::admin_key())
}

pub fn extend_ttl<K>(env: &Env, key: &K)
where
    K: IntoVal<Env, Val>,
{
    env.storage().persistent().extend_ttl(key, 100_000, 500_000);
}

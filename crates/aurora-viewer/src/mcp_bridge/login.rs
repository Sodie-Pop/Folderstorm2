//! Folderstorm: the login generation (folderstorm/plans/mcp-receipts.md,
//! "Exact login binding"). A counter that moves each time the frame thread
//! sees the login change: logged out ↔ logged in, or another login. The
//! waiting calls are bound to the generation they were taken under, so a
//! dialog opened before a relog never runs on the next login.
//!
//! Read from `World::login` once per frame. Each login gives it a new
//! `LoginResponse` (`World::reset` clears it at logout), so the login's
//! identity is that allocation: the tracker keeps a `Weak` to it, which holds
//! the allocation (not the value) and so can never mistake a later login at
//! the same address for this one. The demo session is one login like any
//! other.

use aurora_net::LoginResponse;
use std::sync::{Arc, Weak};

#[derive(Default)]
pub struct Logins {
    /// The login seen last; None while logged out.
    seen: Option<Weak<LoginResponse>>,
    generation: u64,
}

impl Logins {
    /// The generation for `login`, the world's login now: the same as last
    /// frame's while it is the same login.
    pub fn observe(&mut self, login: Option<&Arc<LoginResponse>>) -> u64 {
        let same = match (&self.seen, login) {
            (None, None) => true,
            (Some(seen), Some(now)) => std::ptr::eq(seen.as_ptr(), Arc::as_ptr(now)),
            _ => false,
        };
        if !same {
            self.generation += 1;
            self.seen = login.map(Arc::downgrade);
        }
        self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn login(agent: u128) -> Arc<LoginResponse> {
        Arc::new(LoginResponse {
            agent_id: Uuid::from_u128(agent),
            session_id: Uuid::nil(),
            secure_session_id: Uuid::nil(),
            first_name: String::new(),
            last_name: String::new(),
            sim_ip: std::net::Ipv4Addr::LOCALHOST,
            sim_port: 0,
            circuit_code: 0,
            region_x: 0,
            region_y: 0,
            region_size_x: 256,
            region_size_y: 256,
            seed_capability: String::new(),
            look_at: glam::Vec3::X,
            message: String::new(),
            agent_appearance_service: String::new(),
            inventory_root: Uuid::nil(),
            mfa_hash: None,
            raw: aurora_llsd::Llsd::Undef,
        })
    }

    #[test]
    fn the_generation_moves_with_each_login_change() {
        let mut logins = Logins::default();
        assert_eq!(logins.observe(None), 0, "logged out from the start");
        assert_eq!(logins.observe(None), 0);
        let first = login(1);
        let g1 = logins.observe(Some(&first));
        assert_eq!(g1, 1, "logged in");
        assert_eq!(logins.observe(Some(&first)), g1, "the same login on every frame");
        let g2 = logins.observe(None);
        assert!(g2 > g1, "logged out");
        drop(first);
        // the same character again: another login all the same
        let again = login(1);
        let g3 = logins.observe(Some(&again));
        assert!(g3 > g2);
        // another login without a logout seen in between
        let other = login(2);
        assert!(logins.observe(Some(&other)) > g3);
    }
}

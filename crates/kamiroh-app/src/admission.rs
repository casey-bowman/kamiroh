//! Allowlist enforcement.
//!
//! Called on **every** delivery — not only at conversation-open — so a
//! long-lived connection cannot outlive a revocation.

use kamiroh_domain::allowlist::Allowlist;
use kamiroh_ports::Delivery;

/// The outcome of checking a delivery against the receiving actor's policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Deliver,
    Deny,
}

/// Decide whether `delivery` may reach the actor guarded by `allowlist`.
///
/// Judges the transport-proven origin endpoint only; the claimed sender name
/// plays no part (see the trust model in `ARCHITECTURE.md`).
pub fn admit(allowlist: &Allowlist, delivery: &Delivery) -> Admission {
    if allowlist.allows(&delivery.from.endpoint) {
        Admission::Deliver
    } else {
        Admission::Deny
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kamiroh_domain::actor::{ActorName, Address};
    use kamiroh_domain::endpoint::EndpointId;
    use kamiroh_domain::hex::Hex;
    use kamiroh_domain::vocabulary::{Harness, Message};

    fn address(endpoint: &str, name: &str) -> Address {
        Address::new(
            EndpointId::new(Hex::new(endpoint).unwrap()),
            ActorName::new(name).unwrap(),
        )
    }

    fn delivery(from: Address) -> Delivery {
        Delivery {
            from,
            to: address("00", "receiver"),
            message: Message::Harness(Harness::Ping),
        }
    }

    #[test]
    fn empty_allowlist_denies() {
        let list = Allowlist::empty();
        let d = delivery(address("aa", "sender"));
        assert_eq!(admit(&list, &d), Admission::Deny);
    }

    #[test]
    fn admitted_endpoint_delivers_regardless_of_name() {
        let mut list = Allowlist::empty();
        list.admit(EndpointId::new(Hex::new("aa").unwrap()));
        assert_eq!(
            admit(&list, &delivery(address("aa", "sender"))),
            Admission::Deliver
        );
        // Same endpoint, different claimed name: still delivered — the policy
        // judges endpoints only.
        assert_eq!(
            admit(&list, &delivery(address("aa", "impostor"))),
            Admission::Deliver
        );
    }

    #[test]
    fn revocation_denies_subsequent_deliveries() {
        let mut list = Allowlist::empty();
        let e = EndpointId::new(Hex::new("aa").unwrap());
        list.admit(e.clone());
        assert_eq!(
            admit(&list, &delivery(address("aa", "sender"))),
            Admission::Deliver
        );
        list.revoke(&e);
        assert_eq!(
            admit(&list, &delivery(address("aa", "sender"))),
            Admission::Deny
        );
    }
}

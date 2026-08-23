//! Stock parties.
//!
//! Small [`Party`] implementations the system itself uses — the harness
//! spawns [`EchoParty`] behind new actors, and tests use both.
//!
//! Both keep their shadow protocol state **per peer**, mirroring how the
//! runtime keys conversations (decision 17): one actor may serve several
//! peers at once, and one caller's mid-exchange state must never bleed
//! into another's. (Ruled by Casey at the cucumber errand, 2026-08-23,
//! after the executable specification caught the original single-tenant
//! shadows silently swallowing a second peer's opening turn.) The same
//! keying is what lets [`Party::on_exchange_failed`] fail exactly the
//! conversation that failed and no other.

use std::collections::HashMap;

use kamiroh_domain::actor::Address;
use kamiroh_domain::deadline::FailureCause;
use kamiroh_domain::protocol::TurnState;
use kamiroh_domain::vocabulary::{Response, Turn};
use kamiroh_ports::Party;

/// The simplest party: answers every request by echoing its body, never asks
/// anything of its own — so every exchange with it is a single round,
/// concluded by its `Close`. Serves any number of peers, each conversation
/// tracked separately.
#[derive(Debug, Default)]
pub struct EchoParty {
    /// One shadow machine per peer (decision 17's keying, shadowed).
    states: HashMap<Address, TurnState>,
}

impl EchoParty {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Party for EchoParty {
    async fn on_turn(&mut self, from: &Address, turn: Turn) -> Option<Turn> {
        // Track the exchange with the same domain rulebook everyone uses;
        // an illegal turn gets no reply (the runtime has already validated,
        // so this is belt-and-braces).
        let state = self.states.entry(from.clone()).or_default();
        if state.on_incoming(&turn).is_err() {
            return None;
        }
        let request = turn.request()?.clone();
        let reply = Turn::Close {
            response: Response {
                id: request.id,
                body: request.body,
            },
        };
        state.on_outgoing(&reply).expect("echo reply must be legal");
        Some(reply)
    }

    // A party that tracks shadow machines must also fail them, or a copy
    // diverges from the runtime's authoritative state (decision 23) and a
    // surviving fresh Open is swallowed as MustAnswerFirst. Per-peer
    // keying means exactly the failed conversation's shadow fails —
    // other peers' live exchanges are untouched.
    fn on_exchange_failed(
        &mut self,
        from: &Address,
        _cause: FailureCause,
    ) -> impl std::future::Future<Output = ()> + Send {
        if let Some(state) = self.states.get_mut(from) {
            state.fail();
        }
        async {}
    }
}

/// Per-peer state for [`CountdownParty`]: the shadow machine plus this
/// conversation's own countdown.
#[derive(Debug)]
struct Countdown {
    state: TurnState,
    remaining: u8,
    next_id: u8,
}

/// A multi-round test party: answers each request and, while that
/// conversation's counter is above zero, poses a fresh request of its own
/// (counting down) — producing an exchange of `2n + 1` turns for an
/// initial counter of `n`. Each peer gets its own countdown: two callers
/// count independently.
#[derive(Debug)]
pub struct CountdownParty {
    rounds: u8,
    peers: HashMap<Address, Countdown>,
}

impl CountdownParty {
    pub fn new(rounds: u8) -> Self {
        Self {
            rounds,
            peers: HashMap::new(),
        }
    }
}

impl Party for CountdownParty {
    async fn on_turn(&mut self, from: &Address, turn: Turn) -> Option<Turn> {
        use kamiroh_domain::vocabulary::{Request, RequestId};
        let rounds = self.rounds;
        let peer = self.peers.entry(from.clone()).or_insert_with(|| Countdown {
            state: TurnState::Idle,
            remaining: rounds,
            next_id: 100,
        });
        if peer.state.on_incoming(&turn).is_err() {
            return None;
        }
        let request = turn.request()?.clone();
        let response = Response {
            id: request.id,
            body: request.body,
        };
        let reply = if peer.remaining > 0 {
            peer.remaining -= 1;
            let id = RequestId([peer.next_id; 16]);
            peer.next_id = peer.next_id.wrapping_add(1);
            Turn::Continue {
                response,
                request: Request {
                    id,
                    body: vec![peer.remaining],
                },
            }
        } else {
            Turn::Close { response }
        };
        peer.state
            .on_outgoing(&reply)
            .expect("countdown reply must be legal");
        Some(reply)
    }

    // See EchoParty::on_exchange_failed: fail exactly the failed
    // conversation's shadow, nobody else's.
    fn on_exchange_failed(
        &mut self,
        from: &Address,
        _cause: FailureCause,
    ) -> impl std::future::Future<Output = ()> + Send {
        if let Some(peer) = self.peers.get_mut(from) {
            peer.state.fail();
        }
        async {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kamiroh_domain::actor::ActorName;
    use kamiroh_domain::endpoint::EndpointId;
    use kamiroh_domain::hex::Hex;
    use kamiroh_domain::vocabulary::{Request, RequestId};

    fn address(e: &str, n: &str) -> Address {
        Address::new(
            EndpointId::new(Hex::new(e).unwrap()),
            ActorName::new(n).unwrap(),
        )
    }

    fn open(n: u8) -> Turn {
        Turn::Open {
            request: Request {
                id: RequestId([n; 16]),
                body: vec![n],
            },
        }
    }

    /// The parties' futures never await anything — poll them to completion
    /// with a noop waker.
    fn drive<F: std::future::Future>(f: F) -> F::Output {
        let mut f = std::pin::pin!(f);
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        loop {
            if let std::task::Poll::Ready(out) = f.as_mut().poll(&mut cx) {
                return out;
            }
        }
    }

    /// The cucumber errand's finding, pinned at the unit level: one actor,
    /// two peers, both served — the second caller's Open must not be
    /// refused by the first caller's mid-exchange shadow.
    #[test]
    fn a_countdown_party_serves_two_peers_at_once() {
        let mut party = CountdownParty::new(2);
        let a = address("aa", "app");
        let c = address("cc", "app");

        // Peer A opens; the party continues (mid-exchange with A).
        let reply = drive(party.on_turn(&a, open(1)));
        assert!(matches!(reply, Some(Turn::Continue { .. })));

        // Peer C opens while A's exchange is live: served, not swallowed.
        let reply = drive(party.on_turn(&c, open(2)));
        assert!(
            matches!(reply, Some(Turn::Continue { .. })),
            "the second peer's Open must be answered"
        );
    }

    /// The per-peer half of the failure rule: failing A's exchange leaves
    /// C's live — and A may reopen fresh afterward.
    #[test]
    fn failing_one_peers_exchange_spares_the_other() {
        let mut party = CountdownParty::new(2);
        let a = address("aa", "app");
        let c = address("cc", "app");
        assert!(drive(party.on_turn(&a, open(1))).is_some());
        assert!(drive(party.on_turn(&c, open(2))).is_some());

        // A's exchange fails (say, revoked). C's must be untouched: its
        // response to the party's question is still legal...
        drive(party.on_exchange_failed(&a, FailureCause::Revoked));
        let c_answer = Turn::Close {
            response: Response {
                id: RequestId([100; 16]),
                body: vec![],
            },
        };
        assert!(
            party
                .peers
                .get_mut(&c)
                .unwrap()
                .state
                .on_incoming(&c_answer)
                .is_ok(),
            "C's conversation must survive A's failure"
        );
        // ...and A may open a fresh exchange (the conversation survives).
        assert!(
            drive(party.on_turn(&a, open(3))).is_some(),
            "A's fresh Open after failure must be answered"
        );
    }

    /// Echo serves interleaved peers too (it was accidentally safe before —
    /// now it is safe on purpose, and pinned).
    #[test]
    fn an_echo_party_serves_two_peers_at_once() {
        let mut party = EchoParty::new();
        let a = address("aa", "app");
        let c = address("cc", "app");
        assert!(matches!(
            drive(party.on_turn(&a, open(1))),
            Some(Turn::Close { .. })
        ));
        assert!(matches!(
            drive(party.on_turn(&c, open(2))),
            Some(Turn::Close { .. })
        ));
    }
}

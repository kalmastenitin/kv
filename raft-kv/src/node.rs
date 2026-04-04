// Raft node state machine
use crate::rpc::{RequestVoteArgs, LogEntry, AppendEntriesReply, AppendEntriesArgs, RequestVoteReply};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use rand::Rng;
use crate::log::{Wal, WalRecord};


pub struct RaftNode {

    pub id: u64,         // identity
    pub peers: Vec<u64>, // ids of other index

    // persistent state (must survive restart)
    pub current_term: u64,
    pub voted_for: Option<u64>,
    pub log: Vec<LogEntry>,

    // volatile stae
    pub commit_index: u64,
    pub last_applied: u64,
    pub state: NodeState,

    // leader only volatile state
    pub next_index: HashMap<u64, u64>,      // for each peer, next log index to send
    pub match_index: HashMap<u64, u64>,     // for each peer, highest confirmed index

    pub election_timeout: Duration,
    pub last_heartbeat: std::time::Instant,

    pub wal: Wal,

}

#[derive(Debug, PartialEq, Clone)]
pub enum NodeState {
    Follower,
    Candidate,
    Leader,
}

impl RaftNode {
    pub fn new(id: u64, peers: Vec<u64>, wal_path: &str) -> Self {
        let mut rng = rand::thread_rng();
        let timeout_ms = rng.gen_range(150u64..300u64);
        let records = Wal::recover(wal_path).unwrap_or_default();
      

        let mut node = RaftNode {
            id,
            peers,
            current_term: 0,
            voted_for: None,
            log: Vec::new(),
            commit_index: 0,
            last_applied: 0,
            state: NodeState::Follower,
            next_index: HashMap::new(),
            match_index: HashMap::new(),

            election_timeout: Duration::from_millis(timeout_ms),
            last_heartbeat: Instant::now(),
            wal: Wal::open(wal_path).unwrap(),
        };
        for record in records {
            match record {
                WalRecord::Term(term) => node.current_term = term,
                WalRecord::Vote(vote) => node.voted_for = vote,
                WalRecord::AppendLog(entry) => node.log.push(entry),
            }
        }
        node
    }

    pub fn last_log_index(&self) -> u64 {
        self.log.len() as u64
    }

    pub fn last_log_term(&self) -> u64 {
        self.log.last().map(|e| e.term).unwrap_or(0)
    }

    pub fn handle_request_vote(&mut self, args: RequestVoteArgs) -> RequestVoteReply {
        // rule 1 — candidate is behind, reject
        if args.term < self.current_term {
            return RequestVoteReply { term: self.current_term, vote_granted: false };
        }
        // rule 2 — candidate is ahead, step down
        if args.term > self.current_term {
            self.wal.append(&WalRecord::Term(args.term)).unwrap();
            self.current_term = args.term;
            self.wal.append(&WalRecord::Vote(None)).unwrap();
            self.voted_for = None;
            self.state = NodeState::Follower;
        }

        // rule 3 — check voted_for and log up-to-date
        let can_vote = self.voted_for.is_none() 
            || self.voted_for == Some(args.candidate_id);

        let log_ok = args.last_log_term > self.last_log_term()
            || (args.last_log_term == self.last_log_term() 
                && args.last_log_index >= self.last_log_index());

        if can_vote && log_ok {
            self.wal.append(&WalRecord::Vote(Some(args.candidate_id))).unwrap();

            self.voted_for = Some(args.candidate_id);
            RequestVoteReply { term: self.current_term, vote_granted: true }
        } else {
            RequestVoteReply { term: self.current_term, vote_granted: false }
        }
    }

    pub fn handle_append_entries(&mut self, args: AppendEntriesArgs) -> AppendEntriesReply {
        if args.term < self.current_term {
            return AppendEntriesReply { term: self.current_term, success: false };
        }
        if args.term >= self.current_term {
            self.state = NodeState::Follower;
            self.current_term = args.term;
        }

        if args.prev_log_index > 0 {
            // we must have an entry at prev_log_index with matching term
            let our_term = self.log.get(args.prev_log_index as usize - 1)
                .map(|e| e.term)
                .unwrap_or(0);
            if our_term != args.prev_log_term {
                return AppendEntriesReply { term: self.current_term, success: false };
            }
        }

        for (i, entry) in args.entries.iter().enumerate() {
            let idx = args.prev_log_index as usize + i;
            if idx < self.log.len() {
                if self.log[idx].term != entry.term {
                    self.log.truncate(idx);  // remove conflicting entries
                    self.log.push(entry.clone());
                }
                // else entry already matches — skip
            } else {
                self.log.push(entry.clone());
            }
        }
        if args.leader_commit > self.commit_index {
            self.commit_index = args.leader_commit.min(self.last_log_index());
        }

        return AppendEntriesReply { term: self.current_term, success: true };
     
    }

    pub fn propose(&mut self, command: String) -> Option<u64> {
        // only leader can accept commands
        if self.state != NodeState::Leader {
            return None;
        }
        self.wal.append(&WalRecord::AppendLog(LogEntry {
            term: self.current_term,
            command: command.clone(),
        })).unwrap();
        // append to own log
        self.log.push(LogEntry {
            term: self.current_term,
            command,
        });
        // return the index of the new entry
        Some(self.last_log_index())
    }

    pub fn is_election_timeout(&self) -> bool {
        self.last_heartbeat.elapsed() > self.election_timeout
    }

    pub fn reset_election_timer(&mut self) {
        self.last_heartbeat = std::time::Instant::now();
    }

    pub fn start_election(&mut self) -> Vec<(u64, RequestVoteArgs)> {
        self.wal.append(&WalRecord::Term(self.current_term + 1)).unwrap();
        
        self.wal.append(&WalRecord::Vote(Some(self.id))).unwrap();
        self.voted_for = Some(self.id);
        self.current_term += 1;

        self.state = NodeState::Candidate;

        
        self.reset_election_timer();
        
        self.peers.iter().map(|&peer_id| {
            (peer_id, RequestVoteArgs {
                term: self.current_term,
                candidate_id: self.id,
                last_log_index: self.last_log_index(),
                last_log_term: self.last_log_term(),
            })
        }).collect()
    }

    pub fn handle_vote_reply(&mut self, reply: RequestVoteReply, votes_received: &mut u64) -> bool {
        

        if reply.term > self.current_term {
            self.current_term = reply.term;
            self.state = NodeState::Follower;  // ← must step down
            self.voted_for = None;
            return false;
        };

        if self.state != NodeState::Candidate {
            return false;
        }

        if reply.vote_granted {
            *votes_received += 1;
        };
        if *votes_received > (self.peers.len() as u64 + 1) / 2 {
            self.state = NodeState::Leader;
            for &peer_id in &self.peers {
                self.next_index.insert(peer_id, self.last_log_index() + 1);
                self.match_index.insert(peer_id, 0);
            }
            return true;
        }

        false
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: u64) -> RaftNode {
        let path = format!("/tmp/test_node_{}_{}.wal", id, 
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap().subsec_nanos());
        RaftNode::new(id, vec![1, 2, 3, 4], &path)
    }

    #[test]
    fn test_vote_rejected_lower_term() {
        let mut node = make_node(1);
        node.current_term = 5;
        let reply = node.handle_request_vote(RequestVoteArgs {
            term: 3,
            candidate_id: 2,
            last_log_index: 0,
            last_log_term: 0,
        });
        assert!(!reply.vote_granted);
        assert_eq!(reply.term, 5);
    }

    #[test]
    fn test_vote_granted_higher_term() {
        let mut node = make_node(1);
        node.current_term = 1;
        let reply = node.handle_request_vote(RequestVoteArgs {
            term: 2,
            candidate_id: 2,
            last_log_index: 0,
            last_log_term: 0,
        });
        assert!(reply.vote_granted);
        assert_eq!(node.current_term, 2);
        assert_eq!(node.voted_for, Some(2));
    }

    #[test]
    fn test_vote_rejected_already_voted() {
        // write this one yourself
        // setup: node has already voted for candidate 2 in term 1
        // request: candidate 3 asks for vote in term 1
        // expected: vote not granted
        let mut node = make_node(1);
        node.current_term = 1;
        
        node.handle_request_vote(RequestVoteArgs { 
            term: 1, candidate_id: 1, last_log_index: 0, last_log_term: 0 
        });
        let reply = node.handle_request_vote(RequestVoteArgs { 
            term: 1, candidate_id: 3, last_log_index: 0, last_log_term: 0 
        });
        assert!(!reply.vote_granted);
        assert_eq!(node.voted_for, Some(1)); 
    }

    #[test]
    fn test_append_entries_heartbeat() {
        let mut node = make_node(1);
        node.current_term = 1;
        let reply = node.handle_append_entries(AppendEntriesArgs {
            term: 2,
            leader_id: 2,
            prev_log_index: 0,
            prev_log_term: 0,
            entries: vec![],        // empty = heartbeat
            leader_commit: 0,
        });
        assert!(reply.success);
        assert_eq!(node.current_term, 2);
        assert_eq!(node.state, NodeState::Follower);
    }

    #[test]
    fn test_election_timeout() {
        let node = make_node(1);
        // immediately after creation — should not have timed out
        assert!(!node.is_election_timeout());
    }

    #[test]
    fn test_start_election() {
        let mut node = make_node(1);
        node.current_term = 1;
        let messages = node.start_election();

        assert_eq!(node.current_term, 2);           // term incremented
        assert_eq!(node.state, NodeState::Candidate);
        assert_eq!(node.voted_for, Some(1));        // voted for itself
        assert_eq!(messages.len(), 4);              // one message per peer
        assert!(messages.iter().all(|(_, args)| args.term == 2));
    }

    #[test]
    fn test_become_leader() {
        let mut node = make_node(1);  // peers = [1,2,3,4] — 5 nodes total
        node.current_term = 1;
        node.state = NodeState::Candidate;
        node.voted_for = Some(1);
        
        let mut votes = 1u64;  // already voted for itself
        
        // receive 2 more votes — majority of 5 is 3
        let reply = RequestVoteReply { term: 1, vote_granted: true };
        node.handle_vote_reply(reply.clone(), &mut votes);
        let became_leader = node.handle_vote_reply(reply.clone(), &mut votes);
        
        assert!(became_leader);
        assert_eq!(node.state, NodeState::Leader);
    }

    #[test]
    fn test_propose_as_leader() {
        let mut node = make_node(1);
        node.state = NodeState::Leader;
        node.current_term = 1;

        let idx = node.propose("set name alice".to_string());
        assert_eq!(idx, Some(1));
        assert_eq!(node.log.len(), 1);
        assert_eq!(node.log[0].command, "set name alice");
    }

    #[test]
    fn test_propose_as_follower_rejected() {
        let mut node = make_node(1);
        // state is Follower by default
        let idx = node.propose("set name alice".to_string());
        assert_eq!(idx, None);
        assert_eq!(node.log.len(), 0);
    }

    #[test]
    fn test_wal_recovery() {
        let path = format!("/tmp/test_recovery_{}.wal", 
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap().subsec_nanos());

        // node 1 — write state then drop
        {
            let mut node = RaftNode::new(1, vec![2, 3, 4, 5], &path);
            node.current_term = 3;
            node.wal.append(&WalRecord::Term(3)).unwrap();
            node.wal.append(&WalRecord::Vote(Some(2))).unwrap();
            node.wal.append(&WalRecord::AppendLog(
                crate::rpc::LogEntry { term: 3, command: "set x 1".to_string() }
            )).unwrap();
        } // node dropped here — simulates crash

        // node 2 — recover from same WAL path
        let recovered = RaftNode::new(1, vec![2, 3, 4, 5], &path);
        assert_eq!(recovered.current_term, 3);
        assert_eq!(recovered.voted_for, Some(2));
        assert_eq!(recovered.log.len(), 1);
        assert_eq!(recovered.log[0].command, "set x 1");
    }
}
mod node;
mod rpc;
mod log;

use rpc::{Envelope, RaftMessage, RequestVoteArgs, AppendEntriesArgs};

use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::time;
use node::{RaftNode, NodeState};

type Inbox = mpsc::Sender<Envelope>;

#[tokio::main]
async fn main() {
    let node_ids = vec![1u64, 2, 3, 4, 5];

    // create one inbox channel per node
    let mut inboxes: HashMap<u64, Inbox> = HashMap::new();
    let mut receivers = HashMap::new();

    for &id in &node_ids {
        let (tx, rx) = mpsc::channel::<Envelope>(100);
        inboxes.insert(id, tx);
        receivers.insert(id, rx);
    }

    // spawn one task per node
    for &id in &node_ids {
        let peers: Vec<u64> = node_ids.iter()
            .filter(|&&p| p != id)
            .cloned()
            .collect();

        let rx = receivers.remove(&id).unwrap();
        let inboxes_clone = inboxes.clone();

        tokio::spawn(async move {
            run_node(id, peers, rx, inboxes_clone).await;
        });
    }

    // let cluster run for 5 seconds
    time::sleep(time::Duration::from_secs(1)).await;
    println!("Sending command to cluster...");

    for &id in &node_ids {
        if let Some(inbox) = inboxes.get(&id) {
            let _ = inbox.send(Envelope {
                from: 0,
                to: id,
                message: RaftMessage::ClientCommand("set name alice".to_string()),
            }).await;
        }
    }

    time::sleep(time::Duration::from_secs(4)).await;
    println!("Done");
}

async fn run_node(
    id: u64,
    peers: Vec<u64>,
    mut rx: mpsc::Receiver<Envelope>,
    inboxes: HashMap<u64, Inbox>,
) {
    let mut node = RaftNode::new(id, peers, &format!("/tmp/raft_node_{}.wal", id));
    let mut votes_received = 0u64;

    loop {
        tokio::select! {
            Some(envelope) = rx.recv() => {
            match envelope.message {
                RaftMessage::RequestVote(args) => {
                    // call handle_request_vote
                    // send RequestVoteReply back to envelope.from
                    let reply = node.handle_request_vote(args);
                    let target_id = envelope.from;
                    if let Some(inbox) = inboxes.get(&target_id) {
                        let _ = inbox.send(Envelope {
                            from: id,
                            to: target_id,
                            message: RaftMessage::RequestVoteReply(reply),
                        }).await;
                    }
                }
                RaftMessage::RequestVoteReply(reply) => {
                    // call handle_vote_reply
                    // if became leader — print "Node {id} became leader in term {}"
                    // reset votes_received to 0
                    let became_leader = node.handle_vote_reply(reply, &mut votes_received);
                    if became_leader {
                        println!("Node {} became leader in term {}", id, node.current_term);
                        votes_received = 0;
                    }
                }
                RaftMessage::AppendEntries(args) => {
                    // call handle_append_entries
                    // reset election timer on node
                    // send AppendEntriesReply back to envelope.from
                    let reply = node.handle_append_entries(args);
                    node.reset_election_timer();
                    let target_id = envelope.from;
                    if let Some(inbox) = inboxes.get(&target_id) {
                        let _ = inbox.send(Envelope {
                            from: id,
                            to: target_id,
                            message: RaftMessage::AppendEntriesReply(reply),
                        }).await;
                    }
                }
                RaftMessage::AppendEntriesReply(reply) => {
                    if node.state != NodeState::Leader { continue; }
                    
                    if reply.success {
                        // update match_index and next_index for this peer
                        let peer = envelope.from;
                        
                        // next_index advances to what we just sent
                        node.next_index.insert(peer, node.last_log_index() + 1);
                        node.match_index.insert(peer, node.last_log_index());

                        // check if we can advance commit_index
                        // find highest index replicated on majority
                        let mut indices: Vec<u64> = node.match_index.values().cloned().collect();
                        indices.push(node.last_log_index()); // leader has it too
                        indices.sort();
                        let majority_idx = indices[indices.len() / 2];

                        if majority_idx > node.commit_index 
                            && node.log.get(majority_idx as usize - 1)
                                .map(|e| e.term) == Some(node.current_term) 
                        {
                            node.commit_index = majority_idx;
                            println!("Node {} committed index {} — '{}'", 
                                id, 
                                node.commit_index,
                                node.log[node.commit_index as usize - 1].command);
                        }

                        while node.last_applied < node.commit_index {
                            node.last_applied += 1;
                            let cmd = &node.log[node.last_applied as usize - 1].command;
                            println!("Node {} applying: {}", id, cmd);
                            
                        }
                    } else {
                        // follower rejected — decrement next_index and retry
                        let peer = envelope.from;
                        let next = node.next_index.get(&peer).copied().unwrap_or(1);
                        if next > 1 {
                            node.next_index.insert(peer, next - 1);
                        }
                    }
                }
                RaftMessage::ClientCommand(command) => {
                    if let Some(idx) = node.propose(command) {
                        println!("Node {} accepted command at index {}", id, idx);
                    }
                    // if not leader — silently ignore for now
                }
            }
        }
        _ = tokio::time::sleep(tokio::time::Duration::from_millis(10)) => {
            if node.state == NodeState::Leader {
                // send heartbeat to all peers
                for &peer_id in &node.peers {
                    let next_idx = *node.next_index.get(&peer_id).unwrap_or(&1);
                    
                    // entries to send — everything from next_idx onwards
                    let entries = node.log.get((next_idx as usize - 1)..)
                        .unwrap_or(&[])
                        .to_vec();

                    let prev_log_index = next_idx - 1;
                    let prev_log_term = if prev_log_index > 0 {
                        node.log.get(prev_log_index as usize - 1)
                            .map(|e| e.term)
                            .unwrap_or(0)
                    } else {
                        0
                    };

                    if let Some(inbox) = inboxes.get(&peer_id) {
                        let _ = inbox.send(Envelope {
                            from: id,
                            to: peer_id,
                            message: RaftMessage::AppendEntries(AppendEntriesArgs {
                                term: node.current_term,
                                leader_id: id,
                                prev_log_index,
                                prev_log_term,
                                entries,
                                leader_commit: node.commit_index,
                            }),
                        }).await;
                    }
                }
            } else if node.is_election_timeout() {
                // follower/candidate timed out — start election
                votes_received = 1;
                let messages = node.start_election();
                for (peer_id, args) in messages {
                    if let Some(inbox) = inboxes.get(&peer_id) {
                        let _ = inbox.send(Envelope {
                            from: id,
                            to: peer_id,
                            message: RaftMessage::RequestVote(args),
                        }).await;
                    }
                }
            }
            }
        }
    }
}

mod log;
mod node;
mod rpc;
mod tracer;

use rpc::{AppendEntriesArgs, Envelope, RaftMessage};

use node::{NodeState, RaftNode};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::time;

type Inbox = mpsc::Sender<Envelope>;
type DeadNodes = Arc<Mutex<HashSet<u64>>>;
type CurrentLeader = Arc<Mutex<Option<u64>>>;

#[tokio::main]
async fn main() {
    let node_ids = vec![1u64, 2, 3, 4, 5];
    let dead_nodes: DeadNodes = Arc::new(Mutex::new(HashSet::new()));
    let current_leader: CurrentLeader = Arc::new(Mutex::new(None));

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
        let peers: Vec<u64> = node_ids.iter().filter(|&&p| p != id).cloned().collect();

        let rx = receivers.remove(&id).unwrap();
        let inboxes_clone = inboxes.clone();
        let dead_clone = Arc::clone(&dead_nodes);
        let leader_clone = Arc::clone(&current_leader);

        tokio::spawn(async move {
            run_node(id, peers, rx, inboxes_clone, dead_clone, leader_clone).await;
        });
    }

    // let cluster run for 5 seconds
    time::sleep(time::Duration::from_secs(1)).await;
    println!("Sending command to cluster...");

    for &id in &node_ids {
        if let Some(inbox) = inboxes.get(&id) {
            let _ = inbox
                .send(Envelope {
                    from: 0,
                    to: id,
                    message: RaftMessage::ClientCommand("set name alice".to_string()),
                    trace_context: None,
                })
                .await;
        }
    }

    // scenario 1 — let cluster stabilize
    time::sleep(time::Duration::from_millis(500)).await;

    // === Scenario 1: Kill the leader ===
    println!("\n=== Scenario 1: Kill the leader ===");
    send_command("set name alice", &inboxes, &node_ids).await;

    let leader = current_leader.lock().unwrap().clone();
    if let Some(leader_id) = leader {
        println!("Killing leader: Node {}", leader_id);
        dead_nodes.lock().unwrap().insert(leader_id);
    }
    time::sleep(time::Duration::from_millis(500)).await;
    send_command("set name bob", &inboxes, &node_ids).await;

    // === Scenario 2: Partition minority ===
    println!("\n=== Scenario 2: Partition minority ===");
    dead_nodes.lock().unwrap().insert(4);
    dead_nodes.lock().unwrap().insert(5);
    println!("Killed nodes 4 and 5 — majority (3 nodes) still alive");
    send_command("set city mumbai", &inboxes, &node_ids).await;

    // kill one more — now only 2 alive, no majority
    dead_nodes.lock().unwrap().insert(3);
    println!("Killed node 3 — only 2 nodes alive, no majority possible");
    time::sleep(time::Duration::from_millis(200)).await;
    send_command("set age 30", &inboxes, &node_ids).await;
    println!("(command above should not commit — no majority)");

    // === Scenario 3: Dead node rejoins ===
    println!("\n=== Scenario 3: Revive nodes ===");
    dead_nodes.lock().unwrap().remove(&3);
    dead_nodes.lock().unwrap().remove(&4);
    dead_nodes.lock().unwrap().remove(&5);
    println!("Revived nodes 3, 4, 5");
    time::sleep(time::Duration::from_millis(500)).await;
    send_command("set status alive", &inboxes, &node_ids).await;

    time::sleep(time::Duration::from_secs(2)).await;
    println!("\nDone");
}

async fn run_node(
    id: u64,
    peers: Vec<u64>,
    mut rx: mpsc::Receiver<Envelope>,
    inboxes: HashMap<u64, Inbox>,
    dead_nodes: DeadNodes,
    current_leader: CurrentLeader,
) {
    let mut node = RaftNode::new(id, peers, &format!("/tmp/raft_node_{}.wal", id));
    let mut votes_received = 0u64;

    loop {
        if dead_nodes.lock().unwrap().contains(&id) {
            while rx.try_recv().is_ok() {}
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            continue;
        }

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
                            trace_context: None
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
                        *current_leader.lock().unwrap() = Some(id);  // ← add

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
                            trace_context: None
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
                            trace_context: None
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
                            trace_context: None
                        }).await;
                    }
                }
            }
            }
        }
    }
}

async fn send_command(command: &str, inboxes: &HashMap<u64, Inbox>, node_ids: &[u64]) {
    println!("Sending command: '{}'", command);
    for &id in node_ids {
        if let Some(inbox) = inboxes.get(&id) {
            let _ = inbox
                .send(Envelope {
                    from: 0,
                    to: id,
                    message: RaftMessage::ClientCommand(command.to_string()),
                    trace_context: None,
                })
                .await;
        }
    }
    time::sleep(time::Duration::from_millis(300)).await;
}

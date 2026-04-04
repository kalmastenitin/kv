// RequestVote AppendEntries messages

#[derive(Debug, Clone)]
pub struct RequestVoteArgs {
    pub term: u64,  // candidate's term
    pub candidate_id: u64, // who is requesting vote
    pub last_log_index: u64, // candidate's last log entry index
    pub last_log_term: u64, // candidate's last log entry term
}

#[derive(Debug, Clone)]
pub struct RequestVoteReply {
    pub term: u64,      // current term to update in candidate
    pub vote_granted: bool,
}

// AppendEntries - sent by leader for replication and heartbeats
#[derive(Debug,Clone)]
pub struct AppendEntriesArgs {
    pub term: u64,
    pub leader_id: u64,
    pub prev_log_index: u64,    // index of entry before new
    pub prev_log_term: u64,     // term of perv_log_entry
    pub entries: Vec<LogEntry>, // empty for heartbeats
    pub leader_commit: u64,     // leader's commit index
}

#[derive(Debug, Clone)]
pub struct AppendEntriesReply {
    pub term: u64,
    pub success: bool,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub term: u64,
    pub command: String,        // "set key value" or "del key"
}

#[derive(Debug, Clone)]
pub enum RaftMessage {
    RequestVote(RequestVoteArgs),
    RequestVoteReply(RequestVoteReply),
    AppendEntries(AppendEntriesArgs),
    AppendEntriesReply(AppendEntriesReply),
    ClientCommand(String),
}

#[derive(Debug, Clone)]
pub struct Envelope {
    pub from: u64,
    pub to: u64,
    pub message: RaftMessage,
}
use crate::pubkey::Pubkey;
use crate::system_program;
use crate::transaction_builder::BuilderInstruction;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum SystemInstruction {
    /// Create a new account
    /// * Transaction::keys[0] - source
    /// * Transaction::keys[1] - new account key
    /// * tokens - number of tokens to transfer to the new account
    /// * space - memory to allocate if greater then zero
    /// * program_id - the program id of the new account
    CreateAccount {
        tokens: u64,
        space: u64,
        program_id: Pubkey,
    },
    /// Assign account to a program
    /// * Transaction::keys[0] - account to assign
    Assign { program_id: Pubkey },
    /// Move tokens
    /// * Transaction::keys[0] - source
    /// * Transaction::keys[1] - destination
    Move { tokens: u64 },
}

impl SystemInstruction {
    pub fn new_move(from_id: Pubkey, to_id: Pubkey, tokens: u64) -> BuilderInstruction {
        BuilderInstruction::new(
            system_program::id(),
            &SystemInstruction::Move { tokens },
            vec![(from_id, true), (to_id, false)],
        )
    }
}

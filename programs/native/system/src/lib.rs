use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::native_program::ProgramError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::solana_entrypoint;
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::system_program;

solana_entrypoint!(entrypoint);
pub fn entrypoint(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    _tick_height: u64,
) -> Result<(), ProgramError> {
    if let Ok(syscall) = bincode::deserialize(data) {
        trace!("process_instruction: {:?}", syscall);
        trace!("keyed_accounts: {:?}", keyed_accounts);
        let from = 0;

        // all system instructions require that accounts_keys[0] be a signer
        if keyed_accounts[from].signer_key().is_none() {
            info!("account[from] is unsigned");
            Err(ProgramError::InvalidArgument)?;
        }

        match syscall {
            SystemInstruction::CreateAccount {
                tokens,
                space,
                program_id,
            } => {
                let to = 1;
                if !system_program::check_id(&keyed_accounts[from].account.owner) {
                    info!("CreateAccount: invalid account[from] owner");
                    Err(ProgramError::InvalidArgument)?;
                }

                if space > 0
                    && (!keyed_accounts[to].account.userdata.is_empty()
                        || !system_program::check_id(&keyed_accounts[to].account.owner))
                {
                    info!(
                        "CreateAccount: invalid argument space: {} accounts.userdata.len(): {}",
                        space,
                        keyed_accounts[to].account.userdata.len(),
                    );
                    Err(ProgramError::InvalidArgument)?;
                }
                if tokens > keyed_accounts[from].account.tokens {
                    info!(
                        "CreateAccount: insufficient tokens ({}, need {})",
                        keyed_accounts[from].account.tokens, tokens
                    );
                    Err(ProgramError::ResultWithNegativeTokens)?;
                }
                keyed_accounts[from].account.tokens -= tokens;
                keyed_accounts[to].account.tokens += tokens;
                keyed_accounts[to].account.owner = program_id;
                keyed_accounts[to].account.userdata = vec![0; space as usize];
                keyed_accounts[to].account.executable = false;
            }
            SystemInstruction::Assign { program_id } => {
                if !system_program::check_id(&keyed_accounts[from].account.owner) {
                    Err(ProgramError::AssignOfUnownedAccount)?;
                }
                keyed_accounts[from].account.owner = program_id;
            }
            SystemInstruction::Move { tokens } => {
                let to = 1;

                // bank should be verifying correctness
                if tokens > keyed_accounts[from].account.tokens {
                    info!(
                        "Move: insufficient tokens ({}, need {})",
                        keyed_accounts[from].account.tokens, tokens
                    );
                    Err(ProgramError::ResultWithNegativeTokens)?;
                }
                keyed_accounts[from].account.tokens -= tokens;
                keyed_accounts[to].account.tokens += tokens;
            }
        }
        Ok(())
    } else {
        info!("Invalid transaction instruction userdata: {:?}", data);
        Err(ProgramError::InvalidArgument)
    }
}

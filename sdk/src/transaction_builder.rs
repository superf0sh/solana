//! A library for composing transactions.

use crate::hash::Hash;
use crate::pubkey::Pubkey;
use crate::signature::KeypairUtil;
use crate::transaction::{Instruction, Transaction};
use itertools::Itertools;

pub type BuilderInstruction = Instruction<Pubkey, (Pubkey, bool)>;

fn position(keys: &[Pubkey], key: Pubkey) -> u8 {
    keys.iter().position(|&k| k == key).unwrap() as u8
}

fn create_indexed_instruction(
    ix: &Instruction<Pubkey, (Pubkey, bool)>,
    keys: &[Pubkey],
    program_ids: &[Pubkey],
) -> Instruction<u8, u8> {
    let accounts: Vec<_> = ix
        .accounts
        .iter()
        .map(|&(k, _)| position(keys, k))
        .collect();
    Instruction {
        program_ids_index: position(program_ids, ix.program_ids_index),
        userdata: ix.userdata.clone(),
        accounts,
    }
}

/// A utility for constructing transactions
#[derive(Default)]
pub struct TransactionBuilder {
    fee: u64,
    instructions: Vec<BuilderInstruction>,
}

impl TransactionBuilder {
    /// Create a new TransactionBuilder.
    pub fn new(fee: u64) -> Self {
        Self {
            fee,
            instructions: vec![],
        }
    }

    /// Add an instruction.
    pub fn push(&mut self, instruction: BuilderInstruction) -> &mut Self {
        self.instructions.push(instruction);
        self
    }

    /// Return pubkeys referenced by all instructions, with the ones needing signatures first.
    /// No duplicates and order is preserved.
    fn keys(&self) -> Vec<Pubkey> {
        let mut key_and_signed: Vec<_> = self
            .instructions
            .iter()
            .flat_map(|ix| ix.accounts.iter())
            .collect();
        key_and_signed.sort_by(|x, y| y.1.cmp(&x.1));
        key_and_signed.into_iter().map(|x| x.0).unique().collect()
    }

    /// Return program ids referenced by all instructions.  No duplicates and order is preserved.
    fn program_ids(&self) -> Vec<Pubkey> {
        self.instructions
            .iter()
            .map(|ix| ix.program_ids_index)
            .unique()
            .collect()
    }

    /// Return the instructions, but indexing lists of keys and program ids.
    fn instructions(&self, keys: &[Pubkey], program_ids: &[Pubkey]) -> Vec<Instruction<u8, u8>> {
        self.instructions
            .iter()
            .map(|ix| create_indexed_instruction(ix, keys, program_ids))
            .collect()
    }

    /// Return a signed transaction.
    pub fn sign<T: KeypairUtil>(&self, keypairs: &[&T], last_id: Hash) -> Transaction {
        let keys = self.keys();
        let program_ids = self.program_ids();
        let instructions = self.instructions(&keys, &program_ids);
        for (i, keypair) in keypairs.iter().enumerate() {
            assert_eq!(keypair.pubkey(), keys[i], "keypair-pubkey mismatch");
        }
        let unsigned_keys = &keys[keypairs.len()..];
        Transaction::new_with_instructions(
            keypairs,
            unsigned_keys,
            last_id,
            self.fee,
            program_ids,
            instructions,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signature::{Keypair, KeypairUtil};

    #[test]
    fn test_transaction_builder_unique_program_ids() {
        let program_id0 = Pubkey::default();
        let program_ids = TransactionBuilder::default()
            .push(Instruction::new(program_id0, &0, vec![]))
            .push(Instruction::new(program_id0, &0, vec![]))
            .program_ids();
        assert_eq!(program_ids, vec![program_id0]);
    }

    #[test]
    fn test_transaction_builder_unique_program_ids_not_adjacent() {
        let program_id0 = Pubkey::default();
        let program_id1 = Keypair::new().pubkey();
        let program_ids = TransactionBuilder::default()
            .push(Instruction::new(program_id0, &0, vec![]))
            .push(Instruction::new(program_id1, &0, vec![]))
            .push(Instruction::new(program_id0, &0, vec![]))
            .program_ids();
        assert_eq!(program_ids, vec![program_id0, program_id1]);
    }

    #[test]
    fn test_transaction_builder_unique_program_ids_order_preserved() {
        let program_id0 = Keypair::new().pubkey();
        let program_id1 = Pubkey::default(); // Key less than program_id0
        let program_ids = TransactionBuilder::default()
            .push(Instruction::new(program_id0, &0, vec![]))
            .push(Instruction::new(program_id1, &0, vec![]))
            .push(Instruction::new(program_id0, &0, vec![]))
            .program_ids();
        assert_eq!(program_ids, vec![program_id0, program_id1]);
    }

    #[test]
    fn test_transaction_builder_unique_keys_both_signed() {
        let program_id = Pubkey::default();
        let id0 = Pubkey::default();
        let keys = TransactionBuilder::default()
            .push(Instruction::new(program_id, &0, vec![(id0, true)]))
            .push(Instruction::new(program_id, &0, vec![(id0, true)]))
            .keys();
        assert_eq!(keys, vec![id0]);
    }

    #[test]
    fn test_transaction_builder_unique_keys_one_signed() {
        let program_id = Pubkey::default();
        let id0 = Pubkey::default();
        let keys = TransactionBuilder::default()
            .push(Instruction::new(program_id, &0, vec![(id0, false)]))
            .push(Instruction::new(program_id, &0, vec![(id0, true)]))
            .keys();
        assert_eq!(keys, vec![id0]);
    }

    #[test]
    fn test_transaction_builder_unique_keys_order_preserved() {
        let program_id = Pubkey::default();
        let id0 = Keypair::new().pubkey();
        let id1 = Pubkey::default(); // Key less than id0
        let keys = TransactionBuilder::default()
            .push(Instruction::new(program_id, &0, vec![(id0, false)]))
            .push(Instruction::new(program_id, &0, vec![(id1, false)]))
            .keys();
        assert_eq!(keys, vec![id0, id1]);
    }

    #[test]
    fn test_transaction_builder_unique_keys_not_adjacent() {
        let program_id = Pubkey::default();
        let id0 = Pubkey::default();
        let id1 = Keypair::new().pubkey();
        let keys = TransactionBuilder::default()
            .push(Instruction::new(program_id, &0, vec![(id0, false)]))
            .push(Instruction::new(program_id, &0, vec![(id1, false)]))
            .push(Instruction::new(program_id, &0, vec![(id0, true)]))
            .keys();
        assert_eq!(keys, vec![id0, id1]);
    }

    #[test]
    fn test_transaction_builder_signed_keys_first() {
        let program_id = Pubkey::default();
        let id0 = Pubkey::default();
        let id1 = Keypair::new().pubkey();
        let keys = TransactionBuilder::default()
            .push(Instruction::new(program_id, &0, vec![(id0, false)]))
            .push(Instruction::new(program_id, &0, vec![(id1, true)]))
            .keys();
        assert_eq!(keys, vec![id1, id0]);
    }

    #[test]
    #[should_panic]
    fn test_transaction_builder_missing_key() {
        let keypair = Keypair::new();
        TransactionBuilder::default().sign(&[&keypair], Hash::default());
    }

    #[test]
    #[should_panic]
    fn test_transaction_builder_wrong_key() {
        let program_id = Pubkey::default();
        let keypair0 = Keypair::new();
        let wrong_id = Pubkey::default();
        TransactionBuilder::default()
            .push(Instruction::new(program_id, &0, vec![(wrong_id, true)]))
            .sign(&[&keypair0], Hash::default());
    }

    #[test]
    fn test_transaction_builder_correct_key() {
        let program_id = Pubkey::default();
        let keypair0 = Keypair::new();
        let id0 = keypair0.pubkey();
        let tx = TransactionBuilder::default()
            .push(Instruction::new(program_id, &0, vec![(id0, true)]))
            .sign(&[&keypair0], Hash::default());
        assert_eq!(tx.instructions[0], Instruction::new(0, &0, vec![0]));
    }

    #[test]
    fn test_transaction_builder_fee() {
        let tx = TransactionBuilder::new(42).sign(&Vec::<&Keypair>::new(), Hash::default());
        assert_eq!(tx.fee, 42);
    }

    #[test]
    fn test_transaction_builder_kitchen_sink() {
        let program_id0 = Pubkey::default();
        let program_id1 = Keypair::new().pubkey();
        let id0 = Pubkey::default();
        let keypair1 = Keypair::new();
        let id1 = keypair1.pubkey();
        let tx = TransactionBuilder::default()
            .push(Instruction::new(program_id0, &0, vec![(id0, false)]))
            .push(Instruction::new(program_id1, &0, vec![(id1, true)]))
            .push(Instruction::new(program_id0, &0, vec![(id1, false)]))
            .sign(&[&keypair1], Hash::default());
        assert_eq!(tx.instructions[0], Instruction::new(0, &0, vec![1]));
        assert_eq!(tx.instructions[1], Instruction::new(1, &0, vec![0]));
        assert_eq!(tx.instructions[2], Instruction::new(0, &0, vec![0]));
    }
}

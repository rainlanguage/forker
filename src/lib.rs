use alloy_primitives::{Address, U256};
use alloy_sol_types::SolCall;
use foundry_evm::{
    backend::{Backend, DatabaseExt, LocalForkId},
    executors::{Executor, ExecutorBuilder, RawCallResult},
    fork::{CreateFork, ForkId},
    opts::EvmOpts,
};
use revm::{
    primitives::{Address as Addr, Bytes, Env, TransactTo, U256 as Uint256},
    JournaledState,
};
use std::{any::type_name, collections::HashMap};

// re-export
pub use foundry_evm;
pub use revm;

pub struct ForkTypedReturn<T: SolCall> {
    pub raw: RawCallResult,
    pub typed_return: T::Return,
}

#[derive(Debug)]
pub enum ForkCallError {
    ExecutorError,
    TypedError(String),
}

pub struct Forker {
    pub executor: Executor,
    forks: HashMap<ForkId, LocalForkId>,
}

impl Forker {
    pub async fn new(
        fork_url: &str,
        fork_block_number: Option<u64>,
        gas_limit: Option<u64>,
        env: Option<Env>,
    ) -> Forker {
        let fork_id = ForkId::new(fork_url, fork_block_number);
        let evm_opts = EvmOpts {
            fork_url: Some(fork_url.to_string()),
            fork_block_number,
            env: foundry_evm::opts::Env {
                chain_id: None,
                code_size_limit: None,
                gas_limit: u64::MAX,
                ..Default::default()
            },
            memory_limit: u64::MAX,
            ..Default::default()
        };

        let create_fork = CreateFork {
            url: fork_url.to_string(),
            enable_caching: true,
            env: evm_opts.fork_evm_env(fork_url).await.unwrap().0,
            evm_opts,
        };

        let db = Backend::spawn(Some(create_fork.clone())).await;

        let builder = if let Some(gas) = gas_limit {
            ExecutorBuilder::default()
                .gas_limit(Uint256::from(gas))
                .inspectors(|stack| stack.trace(true).debug(false))
        } else {
            ExecutorBuilder::default().inspectors(|stack| stack.trace(true).debug(false))
        };

        let mut forks_map = HashMap::new();
        forks_map.insert(fork_id, U256::from(1));
        Self {
            executor: builder.build(env.unwrap_or(create_fork.env.clone()), db),
            forks: forks_map,
        }
    }

    /// adds new fork and sets it as active or if the fork already exists, selects it as active
    pub async fn add_or_select(
        &mut self,
        fork_url: &str,
        fork_block_number: Option<u64>,
        env: Option<Env>,
    ) -> Result<(), eyre::Report> {
        let fork_id = ForkId::new(fork_url, fork_block_number);
        let mut journaled_state = JournaledState::new(self.executor.env.cfg.spec_id, vec![]);
        if let Some(local_fork_id) = self.forks.get(&fork_id) {
            if self.executor.backend.is_active_fork(*local_fork_id) {
                Ok(())
            } else {
                self.executor
                    .backend
                    .select_fork(
                        *local_fork_id,
                        &mut env.unwrap_or_default(),
                        &mut journaled_state,
                    )
                    .map(|_| ())
            }
        } else {
            let evm_opts = EvmOpts {
                fork_url: Some(fork_url.to_string()),
                fork_block_number,
                env: foundry_evm::opts::Env {
                    chain_id: None,
                    code_size_limit: None,
                    gas_limit: u64::MAX,
                    ..Default::default()
                },
                memory_limit: u64::MAX,
                ..Default::default()
            };

            let create_fork = CreateFork {
                url: fork_url.to_string(),
                enable_caching: true,
                env: evm_opts.fork_evm_env(fork_url).await.unwrap().0,
                evm_opts,
            };
            self.executor
                .backend
                .create_select_fork(
                    create_fork,
                    &mut env.unwrap_or_default(),
                    &mut journaled_state,
                )
                .map(|_| ())
        }
    }

    pub fn call(
        &mut self,
        from_address: &[u8],
        to_address: &[u8],
        calldata: &[u8],
    ) -> eyre::Result<RawCallResult> {
        if from_address.len() != 20 || to_address.len() != 20 {
            return Err(eyre::Report::msg("invalid address!"));
        }
        let mut env = Env::default();
        env.tx.caller = Addr::from_slice(from_address);
        env.tx.data = Bytes::from(calldata.to_vec());
        env.tx.transact_to = TransactTo::Call(Addr::from_slice(to_address));
        // env.tx.gas_limit = 1000;
        // env.tx.gas_price = U256::from(20000);
        // env.tx.gas_priority_fee = Some(U256::from(20000));

        self.executor.call_raw_with_env(env)
    }

    pub fn write(
        &mut self,
        from_address: &[u8],
        to_address: &[u8],
        calldata: &[u8],
        value: U256,
    ) -> eyre::Result<RawCallResult> {
        if from_address.len() != 20 || to_address.len() != 20 {
            return Err(eyre::Report::msg("invalid address!"));
        }

        self.executor.call_raw_committing(
            Addr::from_slice(from_address),
            Addr::from_slice(to_address),
            Bytes::from(calldata.to_vec()),
            value,
        )
    }

    /// Reads from the forked EVM.
    /// # Arguments
    /// * `executor` - An optional instance of `Executor`.
    /// * `from_address` - The address to call from.
    /// * `to_address` - The address to call to.
    /// * `call` - The call to make.
    /// # Returns
    /// A result containing the raw call result and the typed return.
    pub fn alloy_read<T: SolCall>(
        &mut self,
        from_address: Address,
        to_address: Address,
        call: T,
    ) -> Result<ForkTypedReturn<T>, ForkCallError> {
        // let binding = self.build_executor();

        // let mut executor = match executor {
        //     Some(executor) => executor.clone(),
        //     None => binding,
        // };

        let mut env = Env::default();
        env.tx.caller = from_address.0 .0.into();
        env.tx.data = Bytes::from(call.abi_encode());
        env.tx.transact_to = TransactTo::Call(to_address.0 .0.into());

        let raw = self
            .executor
            .call_raw_with_env(env)
            .map_err(|_e| ForkCallError::ExecutorError)?;

        let typed_return =
            T::abi_decode_returns(raw.result.to_vec().as_slice(), true).map_err(|e| {
                ForkCallError::TypedError(format!(
                    "Call:{:?} Error:{:?} Raw:{:?}",
                    type_name::<T>(),
                    e,
                    raw
                ))
            })?;
        Ok(ForkTypedReturn { raw, typed_return })
    }

    /// Writes to the forked EVM.
    /// # Arguments
    /// * `executor` - An optional instance of `Executor`.
    /// * `from_address` - The address to call from.
    /// * `to_address` - The address to call to.
    /// * `call` - The call to make.
    /// * `value` - The value to send with the call.
    /// # Returns
    /// A result containing the raw call result and the typed return.
    pub fn alloy_write<T: SolCall>(
        &mut self,
        from_address: Address,
        to_address: Address,
        call: T,
        value: U256,
    ) -> Result<ForkTypedReturn<T>, ForkCallError> {
        // let mut binding = self.build_executor();

        // let executor = match executor {
        //     Some(executor) => executor,
        //     None => &mut binding,
        // };

        let raw = self
            .executor
            .call_raw_committing(
                from_address.0 .0.into(),
                to_address.0 .0.into(),
                Bytes::from(call.abi_encode()),
                value,
            )
            .map_err(|_e| ForkCallError::ExecutorError)?;

        let typed_return =
            T::abi_decode_returns(raw.result.to_vec().as_slice(), true).map_err(|e| {
                ForkCallError::TypedError(format!("Call:{:?} Error:{:?}", type_name::<T>(), e))
            })?;
        Ok(ForkTypedReturn { raw, typed_return })
    }
}

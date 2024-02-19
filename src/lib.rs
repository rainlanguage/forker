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
use std::{any::type_name, collections::HashMap, error::Error};

// re-export
pub use foundry_evm;
pub use revm;

pub struct Forker {
    pub executor: Executor,
    forks: HashMap<ForkId, LocalForkId>,
}

impl Forker {
    pub async fn new(
        fork_url: &str,
        fork_block_number: Option<u64>,
        env: Option<Env>,
        gas_limit: Option<u64>,
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

    /// adds new fork and sets it as active or if the fork already exists, selects it as active,
    /// does nothing if the fork is already the active fork.
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
            let default_env = create_fork.env.clone();
            self.executor
                .backend
                .create_select_fork(
                    create_fork,
                    &mut env.unwrap_or(default_env),
                    &mut journaled_state,
                )
                .map(|_| ())
        }
    }

    /// Reads from the forked EVM.
    /// # Arguments
    /// * `from_address` - The address to call from.
    /// * `to_address` - The address to call to.
    /// * `calldata` - The calldata.
    /// # Returns
    /// A result containing the raw call result.
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

    /// Writes to the forked EVM.
    /// # Arguments
    /// * `from_address` - The address to call from.
    /// * `to_address` - The address to call to.
    /// * `calldata` - The calldata.
    /// * `value` - The value to send with the call.
    /// # Returns
    /// A result containing the raw call result.
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

    /// Reads from the forked EVM using alloy typed arguments.
    /// # Arguments
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
    ) -> Result<(RawCallResult, T::Return), ForkCallError> {
        let mut env = Env::default();
        env.tx.caller = from_address.0 .0.into();
        env.tx.data = Bytes::from(call.abi_encode());
        env.tx.transact_to = TransactTo::Call(to_address.0 .0.into());

        let raw = self
            .executor
            .call_raw_with_env(env)?;

        let typed_return =
            T::abi_decode_returns(raw.result.to_vec().as_slice(), true).map_err(|e| {
                ForkCallError::TypedError(format!(
                    "Call:{:?} Error:{:?} Raw:{:?}",
                    type_name::<T>(),
                    e,
                    raw
                ))
            })?;
        Ok(( raw, typed_return ))
    }

    /// Writes to the forked EVM using alloy typed arguments.
    /// # Arguments
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
    ) -> Result<(RawCallResult, T::Return), ForkCallError> {
        let raw = self
            .executor
            .call_raw_committing(
                from_address.0 .0.into(),
                to_address.0 .0.into(),
                Bytes::from(call.abi_encode()),
                value,
            )?;

        let typed_return =
            T::abi_decode_returns(raw.result.to_vec().as_slice(), true).map_err(|e| {
                ForkCallError::TypedError(format!("Call:{:?} Error:{:?}", type_name::<T>(), e))
            })?;
        Ok(( raw, typed_return ))
    }
}


#[derive(Debug)]
pub enum ForkCallError {
    ExecutorError(eyre::Report),
    TypedError(String),
}

impl std::fmt::Display for ForkCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExecutorError(v) => write!(f, "{}", v),
            Self::TypedError(v) => write!(f, "{}", v),
        }
    }
}
impl Error for ForkCallError {}
impl From<eyre::Report> for ForkCallError {
    fn from(value: eyre::Report) -> Self {
        Self::ExecutorError(value)
    }
}

#[cfg(test)]
mod tests {
    // use crate::namespace::CreateNamespace;
    use super::*;
    use alloy_primitives::U256;
    use alloy_sol_types::sol;
    // use rain_interpreter_bindings::{
    //     DeployerISP::iParserCall,
    //     IInterpreterStoreV1::{getCall, setCall},
    // };

    sol! {
        interface IERC20 {
            function balanceOf(address account) external view returns (uint256);
            function transfer(address to, uint256 amount) external returns (bool);
            function allowance(address owner, address spender) external view returns (uint256);
            function approve(address spender, uint256 amount) external returns (bool);
            function transferFrom(address from, address to, uint256 amount) external returns (bool);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_forked_evm_read() {
        let fork_url = "https://rpc.ankr.com/polygon_mumbai";
        let fork_block_number = 45658085u64;
        let forked_evm = Forker::new(
            fork_url.into(),
            Some(fork_block_number),
            None,
            None,
        )
        .await;

        let from_address = Address::default();
        let to_address: Address = "0x0754030e91F316B2d0b992fe7867291E18200A77"
            .parse::<Address>()
            .unwrap();
        let call = iParserCall {};
        let result = forked_evm
            .read(None, from_address, to_address, call)
            .unwrap();
        let parser_address = result.typed_return._0;
        let expected_address = "0x4f8024FB052DbE76b156C6C262Ad27e0F436AF98"
            .parse::<Address>()
            .unwrap();
        assert_eq!(parser_address, expected_address);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_forked_evm_write() {
        let fork_url = "https://rpc.ankr.com/polygon_mumbai";
        let fork_block_number = 45658085u64;
        let forked_evm = Forker::new(
            fork_url.into(),
            Some(fork_block_number),
            None,
            None
        )
        .await;
        // let mut executor = forked_evm.build_executor();
        let from_address = Address::repeat_byte(0x02);
        let to_address: Address = "0xF34e1f2BCeC2baD9c7bE8Aec359691839B784861"
            .parse::<Address>()
            .unwrap();
        let namespace = U256::from(1);
        let key = U256::from(3);
        let value = U256::from(4);
        let _set = forked_evm
            .write(
                from_address,
                to_address,
                setCall {
                    namespace,
                    kvs: vec![key, value],
                },
                U256::from(0),
            )
            .unwrap();

        let fully_quallified_namespace =
            CreateNamespace::qualify_namespace(namespace.into(), from_address);

        let get = forked_evm
            .read(
                Some(&mut executor),
                from_address,
                to_address,
                getCall {
                    namespace: fully_quallified_namespace.into(),
                    key: U256::from(3),
                },
            )
            .unwrap()
            .typed_return
            ._0;
        assert_eq!(value, get);
    }
}

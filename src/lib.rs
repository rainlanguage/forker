use foundry_evm::executor::{
    fork::CreateFork, opts::EvmOpts, Backend, Executor, ExecutorBuilder, RawCallResult,
};
use revm::primitives::{Address, Bytes, Env, TransactTo};

// re-export
pub use foundry_evm;
pub use revm;

pub struct ForkedEvm {
    pub executor: Executor,
}

impl ForkedEvm {
    pub fn new(
        fork_url: &str,
        fork_block_number: Option<u64>,
        gas_limit: Option<u64>,
        env: Option<Env>,
    ) -> ForkedEvm {
        let evm_opts = EvmOpts {
            fork_url: Some(fork_url.to_string()),
            fork_block_number,
            env: foundry_evm::executor::opts::Env {
                chain_id: None,
                code_size_limit: None,
                // gas_price: Some(100),
                gas_limit: u64::MAX,
                ..Default::default()
            },
            ..Default::default()
        };

        let fork_opts = CreateFork {
            url: fork_url.to_string(),
            enable_caching: true,
            env: evm_opts.evm_env_blocking().unwrap(),
            evm_opts,
        };

        let db = Backend::spawn(Some(fork_opts.clone()));

        let builder = if let Some(gs) = gas_limit {
            ExecutorBuilder::default()
                .with_gas_limit(gs.into())
                .with_config(env.unwrap_or(fork_opts.env.clone()))
        } else {
            ExecutorBuilder::default().with_config(env.unwrap_or(fork_opts.env.clone()))
        };

        let executor = builder.build(db);
        Self { executor }
    }

    pub fn call(
        &mut self,
        from_address: &[u8],
        to_address: &[u8],
        calldata: &[u8],
    ) -> eyre::Result<RawCallResult> {
        let mut env = Env::default();
        if from_address.len() != 20 || to_address.len() != 20 {
            return Err(eyre::Report::msg("invalid address!"));
        }
        env.tx.caller = Address::from_slice(from_address);
        env.tx.data = Bytes::from(calldata.to_vec());
        env.tx.transact_to = TransactTo::Call(Address::from_slice(to_address));
        // evn.tx.gas_limit = gas_limit;
        // evn.tx.gas_price = U256::from(20000);
        // evn.tx.gas_priority_fee = Some(U256::from(20000));

        self.executor.call_raw_with_env(env)
    }
}

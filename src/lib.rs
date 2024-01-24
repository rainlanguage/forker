use revm::primitives::{Address, Bytes, Env, TransactTo};
use foundry_evm::executor::{opts::EvmOpts, fork::CreateFork, RawCallResult, Executor, Backend, ExecutorBuilder};

// re-export
pub use revm;
pub use foundry_evm;

pub struct ForkedEvm {
    pub executor: Executor,
}

impl ForkedEvm {
    pub fn new(
        env: Option<Env>,
        fork_url: String,
        fork_block_number: Option<u64>,
        gas_limit: u64,
    ) -> ForkedEvm {
        let evm_opts = EvmOpts {
            fork_url: Some(fork_url.clone()),
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
            url: fork_url,
            enable_caching: true,
            env: evm_opts.evm_env_blocking().unwrap(),
            evm_opts,
        };

        let db = Backend::spawn(Some(fork_opts.clone()));

        let mut builder = ExecutorBuilder::default().with_gas_limit(gas_limit.into());

        if let Some(env) = env {
            builder = builder.with_config(env);
        } else {
            builder = builder.with_config(fork_opts.env.clone());
        }

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
        env.tx.caller = Address::from_slice(from_address);
        env.tx.data = Bytes::from(calldata.to_vec());
        env.tx.transact_to = TransactTo::Call(Address::from_slice(to_address));
        // evn.tx.gas_limit = gas_limit;
        // evn.tx.gas_price = U256::from(20000);
        // evn.tx.gas_priority_fee = Some(U256::from(20000));
        
        self.executor.call_raw_with_env(env)
    }
}

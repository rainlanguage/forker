pub use revm::primitives::{Bytes, Env, U256, Address};
pub use foundry_evm::executor::{opts::EvmOpts, fork::CreateFork, RawCallResult, Executor, Backend, ExecutorBuilder};

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
                gas_price: Some(0),
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

    pub fn call_raw(
        &mut self,
        from: Address,
        to: Address,
        calldata: Bytes,
        value: U256,
    ) -> eyre::Result<RawCallResult> {
        let ethers_from = from.to_fixed_bytes().into();
        let ethers_to = to.to_fixed_bytes().into();
        let ethers_value = ethers::types::U256::from_big_endian(&value.to_be_bytes_vec());
        self.executor.call_raw(ethers_from, ethers_to, calldata, ethers_value)
    }
}

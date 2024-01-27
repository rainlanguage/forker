use foundry_evm::{
    backend::Backend,
    executors::{Executor, ExecutorBuilder, RawCallResult},
    fork::CreateFork,
    opts::EvmOpts,
};
use revm::primitives::{Address, Bytes, Env, TransactTo, U256};

// re-export
pub use foundry_evm;
pub use revm;

pub struct ForkedEvm {
    pub executor: Executor,
}

impl ForkedEvm {
    pub async fn new(
        fork_url: &str,
        fork_block_number: Option<u64>,
        gas_limit: Option<u64>,
        env: Option<Env>,
    ) -> ForkedEvm {
        let evm_opts = EvmOpts {
            fork_url: Some(fork_url.to_string()),
            fork_block_number,
            env: foundry_evm::opts::Env {
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
            env: evm_opts.fork_evm_env(fork_url).await.unwrap().0,
            evm_opts,
        };

        let db = Backend::spawn(Some(fork_opts.clone())).await;
        // new(MultiFork::spawn().await, Some(fork_opts.clone()));

        let builder = if let Some(gas) = gas_limit {
            ExecutorBuilder::default().gas_limit(U256::from(gas))
        } else {
            ExecutorBuilder::default()
        };

        Self {
            executor: builder.build(env.unwrap_or(fork_opts.env.clone()), db),
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
        env.tx.caller = Address::from_slice(from_address);
        env.tx.data = Bytes::from(calldata.to_vec());
        env.tx.transact_to = TransactTo::Call(Address::from_slice(to_address));
        // env.tx.gas_limit = 1000;
        // env.tx.gas_price = U256::from(20000);
        // env.tx.gas_priority_fee = Some(U256::from(20000));

        self.executor.call_raw_with_env(env)
    }
}

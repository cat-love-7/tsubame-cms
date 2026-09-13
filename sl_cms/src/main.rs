#[cfg(all(feature = "on-premises", any(feature = "aws", feature = "gcp", feature = "azure")))]
compile_error!("feature \"on-premises\" and feature \"aws\" cannot be enabled at the same time");
#[cfg(all(feature = "aws", any(feature = "gcp", feature = "azure")))]
compile_error!("feature \"on-premises\" and feature \"aws\" cannot be enabled at the same time");
#[cfg(all(feature = "gcp", any(feature = "azure")))]
compile_error!("feature \"on-premises\" and feature \"aws\" cannot be enabled at the same time");

mod repositories;
mod services;
mod models;

#[cfg(feature = "on-premises")]
mod on_premises;

#[cfg(feature = "on-premises")]
use crate::on_premises::AppModule;


#[cfg(feature = "on-premises")]



#[cfg(feature = "on-premises")]
async fn start() {
    use std::sync::Arc;
    
    let app_module = AppModule::new();
    let state = Arc::new(app_module);
    let _ = on_premises::server::start_server(state).await;
}


#[tokio::main]
async fn main() {
    start().await;
}

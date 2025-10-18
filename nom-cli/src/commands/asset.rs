use nominal_client::NominalClient;
use clap::Subcommand;
use conjure_http::client::AsyncService;
use conjure_object::ResourceIdentifier;
use nominal_api::scout::asset::api::{
    AssetSortOptions, SearchAssetsQuery, SearchAssetsRequest, SortKey,
};
use nominal_api::scout::assets::AssetServiceAsyncClient;

#[derive(Subcommand)]
pub enum AssetCommands {
    /// List all assets
    List,
    Get {
        /// The RID of the asset to retrieve
        rid: String,
    },
}

pub async fn handle(cmd: AssetCommands, client: NominalClient) {
    match cmd {
        AssetCommands::List => {
            let service = AssetServiceAsyncClient::new(client.client);
            let request = SearchAssetsRequest::new(
                AssetSortOptions::builder()
                    .is_descending(true)
                    .sort_key(SortKey::Field(
                        nominal_api::scout::asset::api::SortField::CreatedAt,
                    ))
                    .build(),
                SearchAssetsQuery::SearchText("".to_string()),
            );
            let response = service.search_assets(&client.token, &request).await;
            for asset in response.expect("Failed to fetch assets").results() {
                println!("{:#?}\n", asset);
            }
        }
        AssetCommands::Get { rid } => {
            let service = AssetServiceAsyncClient::new(client.client);
            let resource_id =
                ResourceIdentifier::new(rid.as_str()).expect("Failed to create ResourceIdentifier");
            let asset_rid: nominal_api::scout::rids::api::AssetRid = resource_id.into();
            let rid_set = std::collections::BTreeSet::from([asset_rid]);
            let response = service.get_assets(&client.token, &rid_set).await;

            let asset = response
                .expect("Failed to fetch asset")
                .pop_first()
                .expect("no assets found")
                .1;
            println!("{:#?}\n", asset);
        }
    }
}

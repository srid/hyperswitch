use error_stack::{report, ResultExt};
use masking::Secret;
use router_env::{instrument, tracing};
#[cfg(feature = "accounts_cache")]
use storage_impl::redis::cache::{self, CacheKind, ACCOUNTS_CACHE};

use crate::{
    connection,
    core::errors::{self, CustomResult},
    db::MockDb,
    routes::SessionState,
    services::Store,
    types::domain::{
        self,
        behaviour::{Conversion, ReverseConversion},
    },
};

#[async_trait::async_trait]
pub trait MerchantKeyStoreInterface {
    async fn insert_merchant_key_store(
        &self,
        state: &SessionState,
        merchant_key_store: domain::MerchantKeyStore,
        key: &Secret<Vec<u8>>,
    ) -> CustomResult<domain::MerchantKeyStore, errors::StorageError>;

    async fn get_merchant_key_store_by_merchant_id(
        &self,
        state: &SessionState,
        merchant_id: &str,
        key: &Secret<Vec<u8>>,
    ) -> CustomResult<domain::MerchantKeyStore, errors::StorageError>;

    async fn delete_merchant_key_store_by_merchant_id(
        &self,
        merchant_id: &str,
    ) -> CustomResult<bool, errors::StorageError>;

    #[cfg(feature = "olap")]
    async fn list_multiple_key_stores(
        &self,
        state: &SessionState,
        merchant_ids: Vec<String>,
        key: &Secret<Vec<u8>>,
    ) -> CustomResult<Vec<domain::MerchantKeyStore>, errors::StorageError>;
}

#[async_trait::async_trait]
impl MerchantKeyStoreInterface for Store {
    #[instrument(skip_all)]
    async fn insert_merchant_key_store(
        &self,
        state: &SessionState,
        merchant_key_store: domain::MerchantKeyStore,
        key: &Secret<Vec<u8>>,
    ) -> CustomResult<domain::MerchantKeyStore, errors::StorageError> {
        let conn = connection::pg_connection_write(self).await?;
        merchant_key_store
            .construct_new()
            .await
            .change_context(errors::StorageError::EncryptionError)?
            .insert(&conn)
            .await
            .map_err(|error| report!(errors::StorageError::from(error)))?
            .convert(state, key)
            .await
            .change_context(errors::StorageError::DecryptionError)
    }

    #[instrument(skip_all)]
    async fn get_merchant_key_store_by_merchant_id(
        &self,
        state: &SessionState,
        merchant_id: &str,
        key: &Secret<Vec<u8>>,
    ) -> CustomResult<domain::MerchantKeyStore, errors::StorageError> {
        let fetch_func = || async {
            let conn = connection::pg_connection_read(self).await?;

            diesel_models::merchant_key_store::MerchantKeyStore::find_by_merchant_id(
                &conn,
                merchant_id,
            )
            .await
            .map_err(|error| report!(errors::StorageError::from(error)))
        };

        #[cfg(not(feature = "accounts_cache"))]
        {
            fetch_func()
                .await?
                .convert(state, key)
                .await
                .change_context(errors::StorageError::DecryptionError)
        }

        #[cfg(feature = "accounts_cache")]
        {
            let key_store_cache_key = format!("merchant_key_store_{}", merchant_id);
            cache::get_or_populate_in_memory(
                self,
                &key_store_cache_key,
                fetch_func,
                &ACCOUNTS_CACHE,
            )
            .await?
            .convert(state, key)
            .await
            .change_context(errors::StorageError::DecryptionError)
        }
    }

    #[instrument(skip_all)]
    async fn delete_merchant_key_store_by_merchant_id(
        &self,
        merchant_id: &str,
    ) -> CustomResult<bool, errors::StorageError> {
        let delete_func = || async {
            let conn = connection::pg_connection_write(self).await?;
            diesel_models::merchant_key_store::MerchantKeyStore::delete_by_merchant_id(
                &conn,
                merchant_id,
            )
            .await
            .map_err(|error| report!(errors::StorageError::from(error)))
        };

        #[cfg(not(feature = "accounts_cache"))]
        {
            delete_func().await
        }

        #[cfg(feature = "accounts_cache")]
        {
            let key_store_cache_key = format!("merchant_key_store_{}", merchant_id);
            cache::publish_and_redact(
                self,
                CacheKind::Accounts(key_store_cache_key.into()),
                delete_func,
            )
            .await
        }
    }

    #[cfg(feature = "olap")]
    #[instrument(skip_all)]
    async fn list_multiple_key_stores(
        &self,
        state: &SessionState,
        merchant_ids: Vec<String>,
        key: &Secret<Vec<u8>>,
    ) -> CustomResult<Vec<domain::MerchantKeyStore>, errors::StorageError> {
        let fetch_func = || async {
            let conn = connection::pg_connection_read(self).await?;

            diesel_models::merchant_key_store::MerchantKeyStore::list_multiple_key_stores(
                &conn,
                merchant_ids,
            )
            .await
            .map_err(|error| report!(errors::StorageError::from(error)))
        };

        futures::future::try_join_all(fetch_func().await?.into_iter().map(|key_store| async {
            key_store
                .convert(state, key)
                .await
                .change_context(errors::StorageError::DecryptionError)
        }))
        .await
    }
}

#[async_trait::async_trait]
impl MerchantKeyStoreInterface for MockDb {
    async fn insert_merchant_key_store(
        &self,
        state: &SessionState,
        merchant_key_store: domain::MerchantKeyStore,
        key: &Secret<Vec<u8>>,
    ) -> CustomResult<domain::MerchantKeyStore, errors::StorageError> {
        let mut locked_merchant_key_store = self.merchant_key_store.lock().await;

        if locked_merchant_key_store
            .iter()
            .any(|merchant_key| merchant_key.merchant_id == merchant_key_store.merchant_id)
        {
            Err(errors::StorageError::DuplicateValue {
                entity: "merchant_key_store",
                key: Some(merchant_key_store.merchant_id.clone()),
            })?;
        }

        let merchant_key = Conversion::convert(merchant_key_store)
            .await
            .change_context(errors::StorageError::MockDbError)?;
        locked_merchant_key_store.push(merchant_key.clone());

        merchant_key
            .convert(state, key)
            .await
            .change_context(errors::StorageError::DecryptionError)
    }

    async fn get_merchant_key_store_by_merchant_id(
        &self,
        state: &SessionState,
        merchant_id: &str,
        key: &Secret<Vec<u8>>,
    ) -> CustomResult<domain::MerchantKeyStore, errors::StorageError> {
        self.merchant_key_store
            .lock()
            .await
            .iter()
            .find(|merchant_key| merchant_key.merchant_id == merchant_id)
            .cloned()
            .ok_or(errors::StorageError::ValueNotFound(String::from(
                "merchant_key_store",
            )))?
            .convert(state, key)
            .await
            .change_context(errors::StorageError::DecryptionError)
    }

    async fn delete_merchant_key_store_by_merchant_id(
        &self,
        merchant_id: &str,
    ) -> CustomResult<bool, errors::StorageError> {
        let mut merchant_key_stores = self.merchant_key_store.lock().await;
        let index = merchant_key_stores
            .iter()
            .position(|mks| mks.merchant_id == merchant_id)
            .ok_or(errors::StorageError::ValueNotFound(format!(
                "No merchant key store found for merchant_id = {}",
                merchant_id
            )))?;
        merchant_key_stores.remove(index);
        Ok(true)
    }

    #[cfg(feature = "olap")]
    async fn list_multiple_key_stores(
        &self,
        state: &SessionState,
        merchant_ids: Vec<String>,
        key: &Secret<Vec<u8>>,
    ) -> CustomResult<Vec<domain::MerchantKeyStore>, errors::StorageError> {
        let merchant_key_stores = self.merchant_key_store.lock().await;
        futures::future::try_join_all(
            merchant_key_stores
                .iter()
                .filter(|merchant_key| merchant_ids.contains(&merchant_key.merchant_id))
                .map(|merchant_key| async {
                    merchant_key
                        .to_owned()
                        .convert(state, key)
                        .await
                        .change_context(errors::StorageError::DecryptionError)
                }),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use time::macros::datetime;
    use tokio::sync::oneshot;

    use crate::{
        db::{merchant_key_store::MerchantKeyStoreInterface, MasterKeyInterface, MockDb},
        routes::{
            self,
            app::{settings::Settings, StorageImpl},
        },
        services,
        types::domain,
    };

    #[allow(clippy::unwrap_used)]
    #[tokio::test]
    async fn test_mock_db_merchant_key_store_interface() {
        let conf = Settings::new().expect("invalid settings");
        let tx: oneshot::Sender<()> = oneshot::channel().0;
        let app_state = Box::pin(routes::AppState::with_storage(
            conf,
            StorageImpl::PostgresqlTest,
            tx,
            Box::new(services::MockApiClient),
        ))
        .await;
        let state = &Arc::new(app_state)
            .get_session_state("public", || {})
            .unwrap();
        #[allow(clippy::expect_used)]
        let mock_db = MockDb::new(&redis_interface::RedisSettings::default())
            .await
            .expect("Failed to create mock DB");
        let master_key = mock_db.get_master_key();
        let merchant_id = "merchant1";
        let identifier =
            domain::Identifier::Merchant(String::from_utf8_lossy(master_key).to_string());
        let merchant_key1 = mock_db
            .insert_merchant_key_store(
                state,
                domain::MerchantKeyStore {
                    merchant_id: merchant_id.into(),
                    key: domain::types::encrypt(
                        state,
                        services::generate_aes256_key().unwrap().to_vec().into(),
                        identifier.clone(),
                    )
                    .await
                    .unwrap(),
                    created_at: datetime!(2023-02-01 0:00),
                },
                &master_key.to_vec().into(),
            )
            .await
            .unwrap();

        let found_merchant_key1 = mock_db
            .get_merchant_key_store_by_merchant_id(state, merchant_id, &master_key.to_vec().into())
            .await
            .unwrap();

        assert_eq!(found_merchant_key1.merchant_id, merchant_key1.merchant_id);
        assert_eq!(found_merchant_key1.key, merchant_key1.key);

        let insert_duplicate_merchant_key1_result = mock_db
            .insert_merchant_key_store(
                state,
                domain::MerchantKeyStore {
                    merchant_id: merchant_id.into(),
                    key: domain::types::encrypt(
                        state,
                        services::generate_aes256_key().unwrap().to_vec().into(),
                        identifier.clone(),
                    )
                    .await
                    .unwrap(),
                    created_at: datetime!(2023-02-01 0:00),
                },
                &master_key.to_vec().into(),
            )
            .await;
        assert!(insert_duplicate_merchant_key1_result.is_err());

        let find_non_existent_merchant_key_result = mock_db
            .get_merchant_key_store_by_merchant_id(
                state,
                "non_existent",
                &master_key.to_vec().into(),
            )
            .await;
        assert!(find_non_existent_merchant_key_result.is_err());

        let find_merchant_key_with_incorrect_master_key_result = mock_db
            .get_merchant_key_store_by_merchant_id(state, merchant_id, &vec![0; 32].into())
            .await;
        assert!(find_merchant_key_with_incorrect_master_key_result.is_err());
    }
}

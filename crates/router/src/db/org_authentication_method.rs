use diesel_models::{
    enums,
    org_authentication_method::{self as storage},
};
use error_stack::{report, ResultExt};
use router_env::{instrument, tracing};

use super::MockDb;
use crate::{
    connection,
    core::errors::{self, CustomResult},
    services::Store,
};

#[async_trait::async_trait]
pub trait OrgAuthenticationMethodInterface {
    async fn insert_org_authentication_method(
        &self,
        org_authentication_method: storage::OrgAuthenticationMethodNew,
    ) -> CustomResult<storage::OrgAuthenticationMethod, errors::StorageError>;

    async fn get_org_authentication_methods_details(
        &self,
        org_id: &str,
    ) -> CustomResult<Vec<storage::OrgAuthenticationMethod>, errors::StorageError>;

    async fn update_org_authentication_method(
        &self,
        org_id: &str,
        auth_method: enums::AuthMethod,
        org_authentication_method_update: storage::OrgAuthenticationMethodUpdate,
    ) -> CustomResult<storage::OrgAuthenticationMethod, errors::StorageError>;
}

#[async_trait::async_trait]
impl OrgAuthenticationMethodInterface for Store {
    #[instrument(skip_all)]
    async fn insert_org_authentication_method(
        &self,
        org_authentication_method: storage::OrgAuthenticationMethodNew,
    ) -> CustomResult<storage::OrgAuthenticationMethod, errors::StorageError> {
        let conn = connection::pg_connection_write(self).await?;
        org_authentication_method
            .insert(&conn)
            .await
            .map_err(|error| report!(errors::StorageError::from(error)))
    }

    #[instrument(skip_all)]
    async fn get_org_authentication_methods_details(
        &self,
        org_id: &str,
    ) -> CustomResult<Vec<storage::OrgAuthenticationMethod>, errors::StorageError> {
        let conn = connection::pg_connection_write(self).await?;
        storage::OrgAuthenticationMethod::get_org_authentication_methods_details(&conn, org_id)
            .await
            .map_err(|error| report!(errors::StorageError::from(error)))
    }

    #[instrument(skip_all)]
    async fn update_org_authentication_method(
        &self,
        org_id: &str,
        auth_method: enums::AuthMethod,
        org_authentication_method_update: storage::OrgAuthenticationMethodUpdate,
    ) -> CustomResult<storage::OrgAuthenticationMethod, errors::StorageError> {
        let conn = connection::pg_connection_write(self).await?;
        storage::OrgAuthenticationMethod::update_org_authentication_method(
            &conn,
            org_id,
            auth_method,
            org_authentication_method_update,
        )
        .await
        .map_err(|error| report!(errors::StorageError::from(error)))
    }
}

#[async_trait::async_trait]
impl OrgAuthenticationMethodInterface for MockDb {
    async fn insert_org_authentication_method(
        &self,
        org_authentication_method: storage::OrgAuthenticationMethodNew,
    ) -> CustomResult<storage::OrgAuthenticationMethod, errors::StorageError> {
        let mut org_authentication_methods = self.org_authentication_methods.lock().await;
        if org_authentication_methods
            .iter()
            .any(|auth_method_inner| auth_method_inner.org_id == org_authentication_method.org_id)
        {
            Err(errors::StorageError::DuplicateValue {
                entity: "role_id",
                key: None,
            })?
        }
        let org_authentication_method = storage::OrgAuthenticationMethod {
            id: i32::try_from(org_authentication_methods.len())
                .change_context(errors::StorageError::MockDbError)?,
            org_id: org_authentication_method.org_id,
            auth_method: org_authentication_method.auth_method,
            auth_config: org_authentication_method.auth_config,
            created_at: org_authentication_method.created_at,
            last_modified_at: org_authentication_method.last_modified_at,
        };

        org_authentication_methods.push(org_authentication_method.clone());
        Ok(org_authentication_method)
    }

    async fn get_org_authentication_methods_details(
        &self,
        org_id: &str,
    ) -> CustomResult<Vec<storage::OrgAuthenticationMethod>, errors::StorageError> {
        let org_authentication_methods = self.org_authentication_methods.lock().await;

        let org_authentication_methods_list: Vec<_> = org_authentication_methods
            .iter()
            .filter(|auth_method_inner| auth_method_inner.org_id == org_id)
            .cloned()
            .collect();
        if org_authentication_methods_list.is_empty() {
            return Err(errors::StorageError::ValueNotFound(format!(
                "No org authentication found for org_id = {}",
                org_id
            ))
            .into());
        }

        Ok(org_authentication_methods_list)
    }

    async fn update_org_authentication_method(
        &self,
        org_id: &str,
        auth_method: enums::AuthMethod,
        org_authentication_method_update: storage::OrgAuthenticationMethodUpdate,
    ) -> CustomResult<storage::OrgAuthenticationMethod, errors::StorageError> {
        let mut org_authentication_methods = self.org_authentication_methods.lock().await;
        org_authentication_methods
            .iter_mut()
            .find(|auth_method_inner| {
                auth_method_inner.org_id == org_id && auth_method_inner.auth_method == auth_method
            })
            .map(|auth_method_inner| {
                *auth_method_inner = match org_authentication_method_update {
                    storage::OrgAuthenticationMethodUpdate::UpdateAuthConfig { auth_config } => {
                        storage::OrgAuthenticationMethod {
                            auth_config,
                            last_modified_at: common_utils::date_time::now(),
                            ..auth_method_inner.to_owned()
                        }
                    }
                };
                auth_method_inner.to_owned()
            })
            .ok_or(
                errors::StorageError::ValueNotFound(format!(
                    "No authentication method available for the org = {org_id}"
                ))
                .into(),
            )
    }
}

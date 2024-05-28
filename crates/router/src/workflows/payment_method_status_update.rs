use common_utils::ext_traits::ValueExt;
// use router_env::logger;
use scheduler::{
    consumer::types::process_data, utils as pt_utils, workflows::ProcessTrackerWorkflow,
};

use crate::{
    errors,
    logger::error,
    routes::{metrics, AppState},
    types::storage::{self, PaymentMethodStatusTrackingData},
};

pub struct PaymentMethodStatusUpdateWorkflow;

#[async_trait::async_trait]
impl ProcessTrackerWorkflow<AppState> for PaymentMethodStatusUpdateWorkflow {
    async fn execute_workflow<'a>(
        &'a self,
        state: &'a AppState,
        process: storage::ProcessTracker,
    ) -> Result<(), errors::ProcessTrackerError> {
        let db = &*state.store;
        let tracking_data: PaymentMethodStatusTrackingData = process
            .tracking_data
            .clone()
            .parse_value("PaymentMethodStatusTrackingData")?;

        let retry_count = process.retry_count;
        let pm_id = tracking_data.payment_method_id;
        let prev_pm_status = tracking_data.prev_status;
        let curr_pm_status = tracking_data.curr_status;
        let merchant_id = tracking_data.merchant_id;

        let key_store = state
            .store
            .get_merchant_key_store_by_merchant_id(
                merchant_id.as_str(),
                &state.store.get_master_key().to_vec().into(),
            )
            .await?;

        let merchant_account = db
            .find_merchant_account_by_merchant_id(merchant_id.as_str(), &key_store)
            .await?;

        let payment_method = db
            .find_payment_method(pm_id.as_str(), merchant_account.storage_scheme)
            .await?;

        if payment_method.status != prev_pm_status {
            return db
                .as_scheduler()
                .finish_process_with_business_status(
                    process,
                    "PROCESS_ALREADY_COMPLETED".to_string(),
                )
                .await
                .map_err(Into::<errors::ProcessTrackerError>::into);
        }

        let pm_update = storage::PaymentMethodUpdate::StatusUpdate {
            status: Some(curr_pm_status),
        };

        let res = db
            .update_payment_method(payment_method, pm_update, merchant_account.storage_scheme)
            .await
            .map_err(errors::ProcessTrackerError::EStorageError);

        if let Ok(_pm) = res {
            db.as_scheduler()
                .finish_process_with_business_status(process, "COMPLETED_BY_PT".to_string())
                .await?;
        } else {
            let mapping = process_data::PaymentMethodsPTMapping::default();
            let time_delta = if retry_count == 0 {
                Some(mapping.default_mapping.start_after)
            } else {
                pt_utils::get_delay(retry_count + 1, &mapping.default_mapping.frequencies)
            };

            let schedule_time = pt_utils::get_time_from_delta(time_delta);

            match schedule_time {
                Some(s_time) => db
                    .as_scheduler()
                    .retry_process(process, s_time)
                    .await
                    .map_err(Into::<errors::ProcessTrackerError>::into)?,
                None => db
                    .as_scheduler()
                    .finish_process_with_business_status(process, "RETRIES_EXCEEDED".to_string())
                    .await
                    .map_err(Into::<errors::ProcessTrackerError>::into)?,
            };
        };

        Ok(())
    }

    async fn error_handler<'a>(
        &'a self,
        _state: &'a AppState,
        process: storage::ProcessTracker,
        _error: errors::ProcessTrackerError,
    ) -> errors::CustomResult<(), errors::ProcessTrackerError> {
        error!(%process.id, "Failed while executing workflow");
        Ok(())
    }
}

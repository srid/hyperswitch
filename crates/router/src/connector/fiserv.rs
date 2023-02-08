mod transformers;

use std::fmt::Debug;

use base64::Engine;
use error_stack::ResultExt;
use ring::hmac;
use time::OffsetDateTime;
use transformers as fiserv;
use uuid::Uuid;

use crate::{
    configs::settings,
    consts,
    core::{
        errors::{self, CustomResult},
        payments,
    },
    headers, logger, services,
    types::{
        self,
        api::{self, ConnectorCommon},
    },
    utils::{self, BytesExt},
};

#[derive(Debug, Clone)]
pub struct Fiserv;

impl Fiserv {
    pub fn generate_authorization_signature(
        &self,
        auth: fiserv::FiservAuthType,
        request_id: &str,
        payload: &str,
        timestamp: i128,
    ) -> CustomResult<String, errors::ConnectorError> {
        let fiserv::FiservAuthType {
            api_key,
            api_secret,
            ..
        } = auth;
        let raw_signature = format!("{api_key}{request_id}{timestamp}{payload}");

        let key = hmac::Key::new(hmac::HMAC_SHA256, api_secret.as_bytes());
        let signature_value =
            consts::BASE64_ENGINE.encode(hmac::sign(&key, raw_signature.as_bytes()).as_ref());
        Ok(signature_value)
    }
}

impl ConnectorCommon for Fiserv {
    fn id(&self) -> &'static str {
        "fiserv"
    }

    fn common_get_content_type(&self) -> &'static str {
        "application/json"
    }

    fn base_url<'a>(&self, connectors: &'a settings::Connectors) -> &'a str {
        connectors.fiserv.base_url.as_ref()
    }
    fn build_error_response(
        &self,
        res: types::Response,
    ) -> CustomResult<types::ErrorResponse, errors::ConnectorError> {
        let response: fiserv::ErrorResponse = res
            .response
            .parse_struct("Fiserv ErrorResponse")
            .change_context(errors::ConnectorError::ResponseDeserializationFailed)?;

        let fiserv::ErrorResponse { error, details } = response;

        let message = match (error, details) {
            (Some(err), _) => err
                .iter()
                .map(|v| v.message.clone())
                .collect::<Vec<String>>()
                .join(""),
            (None, Some(err_details)) => err_details
                .iter()
                .map(|v| v.message.clone())
                .collect::<Vec<String>>()
                .join(""),
            (None, None) => consts::NO_ERROR_MESSAGE.to_string(),
        };

        Ok(types::ErrorResponse {
            status_code: res.status_code,
            code: consts::NO_ERROR_CODE.to_string(),
            message,
            reason: None,
        })
    }
}

impl api::ConnectorAccessToken for Fiserv {}

impl
    services::ConnectorIntegration<
        api::AccessTokenAuth,
        types::AccessTokenRequestData,
        types::AccessToken,
    > for Fiserv
{
    // Not Implemented (R)
}

impl api::Payment for Fiserv {}

impl api::PreVerify for Fiserv {}

#[allow(dead_code)]
impl
    services::ConnectorIntegration<
        api::Verify,
        types::VerifyRequestData,
        types::PaymentsResponseData,
    > for Fiserv
{
}

impl api::PaymentVoid for Fiserv {}

#[allow(dead_code)]
impl
    services::ConnectorIntegration<
        api::Void,
        types::PaymentsCancelData,
        types::PaymentsResponseData,
    > for Fiserv
{
    fn get_headers(
        &self,
        req: &types::PaymentsCancelRouterData,
        _connectors: &settings::Connectors,
    ) -> CustomResult<Vec<(String, String)>, errors::ConnectorError> {
        let timestamp = OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
        let auth: fiserv::FiservAuthType =
            fiserv::FiservAuthType::try_from(&req.connector_auth_type)?;
        let api_key = auth.api_key.clone();

        let fiserv_req = self
            .get_request_body(req)?
            .ok_or(errors::ConnectorError::RequestEncodingFailed)?;
        let client_request_id = Uuid::new_v4().to_string();
        let hmac = self
            .generate_authorization_signature(auth, &client_request_id, &fiserv_req, timestamp)
            .change_context(errors::ConnectorError::RequestEncodingFailed)?;
        let headers = vec![
            (
                headers::CONTENT_TYPE.to_string(),
                types::PaymentsAuthorizeType::get_content_type(self).to_string(),
            ),
            ("Client-Request-Id".to_string(), client_request_id),
            ("Auth-Token-Type".to_string(), "HMAC".to_string()),
            ("Api-Key".to_string(), api_key),
            ("Timestamp".to_string(), timestamp.to_string()),
            ("Authorization".to_string(), hmac),
        ];
        Ok(headers)
    }

    fn get_content_type(&self) -> &'static str {
        "application/json"
    }

    fn get_url(
        &self,
        _req: &types::PaymentsCancelRouterData,
        connectors: &settings::Connectors,
    ) -> CustomResult<String, errors::ConnectorError> {
        Ok(format!(
            "{}ch/payments/v1/cancels", //The docs has this url wrong, cancels is the working endpoint
            connectors.fiserv.base_url
        ))
    }

    fn get_request_body(
        &self,
        req: &types::PaymentsCancelRouterData,
    ) -> CustomResult<Option<String>, errors::ConnectorError> {
        let connector_req = fiserv::FiservCancelRequest::try_from(req)?;
        let fiserv_req =
            utils::Encode::<fiserv::FiservCancelRequest>::encode_to_string_of_json(&connector_req)
                .change_context(errors::ConnectorError::RequestEncodingFailed)?;
        println!("cancellllll{:?}", fiserv_req);
        Ok(Some(fiserv_req))
    }

    fn build_request(
        &self,
        req: &types::PaymentsCancelRouterData,
        connectors: &settings::Connectors,
    ) -> CustomResult<Option<services::Request>, errors::ConnectorError> {
        let request = Some(
            services::RequestBuilder::new()
                .method(services::Method::Post)
                .url(&types::PaymentsVoidType::get_url(self, req, connectors)?)
                .headers(types::PaymentsVoidType::get_headers(self, req, connectors)?)
                .body(types::PaymentsVoidType::get_request_body(self, req)?)
                .build(),
        );

        Ok(request)
    }

    fn handle_response(
        &self,
        data: &types::PaymentsCancelRouterData,
        res: types::Response,
    ) -> CustomResult<types::PaymentsCancelRouterData, errors::ConnectorError> {
        let response: fiserv::FiservPaymentsResponse = res
            .response
            .parse_struct("Fiserv PaymentResponse")
            .change_context(errors::ConnectorError::ResponseDeserializationFailed)?;
        types::ResponseRouterData {
            response,
            data: data.clone(),
            http_code: res.status_code,
        }
        .try_into()
        .change_context(errors::ConnectorError::ResponseHandlingFailed)
    }

    fn get_error_response(
        &self,
        res: types::Response,
    ) -> CustomResult<types::ErrorResponse, errors::ConnectorError> {
        self.build_error_response(res)
    }
}

impl api::PaymentSync for Fiserv {}

#[allow(dead_code)]
impl
    services::ConnectorIntegration<api::PSync, types::PaymentsSyncData, types::PaymentsResponseData>
    for Fiserv
{
    fn get_headers(
        &self,
        req: &types::PaymentsSyncRouterData,
        _connectors: &settings::Connectors,
    ) -> CustomResult<Vec<(String, String)>, errors::ConnectorError> {
        let timestamp = OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
        let auth: fiserv::FiservAuthType =
            fiserv::FiservAuthType::try_from(&req.connector_auth_type)?;
        let api_key = auth.api_key.clone();

        let fiserv_req = self
            .get_request_body(req)?
            .ok_or(errors::ConnectorError::RequestEncodingFailed)?;
        let client_request_id = Uuid::new_v4().to_string();
        let hmac = self
            .generate_authorization_signature(auth, &client_request_id, &fiserv_req, timestamp)
            .change_context(errors::ConnectorError::RequestEncodingFailed)?;
        let headers = vec![
            (
                headers::CONTENT_TYPE.to_string(),
                types::PaymentsAuthorizeType::get_content_type(self).to_string(),
            ),
            ("Client-Request-Id".to_string(), client_request_id),
            ("Auth-Token-Type".to_string(), "HMAC".to_string()),
            ("Api-Key".to_string(), api_key),
            ("Timestamp".to_string(), timestamp.to_string()),
            ("Authorization".to_string(), hmac),
        ];
        Ok(headers)
    }

    fn get_content_type(&self) -> &'static str {
        "application/json"
    }

    fn get_url(
        &self,
        _req: &types::PaymentsSyncRouterData,
        connectors: &settings::Connectors,
    ) -> CustomResult<String, errors::ConnectorError> {
        Ok(format!(
            "{}ch/payments/v1/transaction-inquiry",
            connectors.fiserv.base_url
        ))
    }

    fn get_request_body(
        &self,
        req: &types::PaymentsSyncRouterData,
    ) -> CustomResult<Option<String>, errors::ConnectorError> {
        let connector_req = fiserv::FiservSyncRequest::try_from(req)?;
        let fiserv_req =
            utils::Encode::<fiserv::FiservSyncRequest>::encode_to_string_of_json(&connector_req)
                .change_context(errors::ConnectorError::RequestEncodingFailed)?;
        Ok(Some(fiserv_req))
    }

    fn build_request(
        &self,
        req: &types::PaymentsSyncRouterData,
        connectors: &settings::Connectors,
    ) -> CustomResult<Option<services::Request>, errors::ConnectorError> {
        let request = Some(
            services::RequestBuilder::new()
                .method(services::Method::Post)
                .url(&types::PaymentsSyncType::get_url(self, req, connectors)?)
                .headers(types::PaymentsSyncType::get_headers(self, req, connectors)?)
                .body(types::PaymentsSyncType::get_request_body(self, req)?)
                .build(),
        );
        Ok(request)
    }

    fn handle_response(
        &self,
        data: &types::PaymentsSyncRouterData,
        res: types::Response,
    ) -> CustomResult<types::PaymentsSyncRouterData, errors::ConnectorError> {
        let response: fiserv::FiservPaymentsResponse = res
            .response
            .parse_struct("Fiserv Payment Response")
            .change_context(errors::ConnectorError::ResponseDeserializationFailed)?;
        types::ResponseRouterData {
            response,
            data: data.clone(),
            http_code: res.status_code,
        }
        .try_into()
        .change_context(errors::ConnectorError::ResponseHandlingFailed)
    }

    fn get_error_response(
        &self,
        res: types::Response,
    ) -> CustomResult<types::ErrorResponse, errors::ConnectorError> {
        self.build_error_response(res)
    }
}

impl api::PaymentCapture for Fiserv {}
impl
    services::ConnectorIntegration<
        api::Capture,
        types::PaymentsCaptureData,
        types::PaymentsResponseData,
    > for Fiserv
{
    fn get_headers(
        &self,
        req: &types::PaymentsCaptureRouterData,
        _connectors: &settings::Connectors,
    ) -> CustomResult<Vec<(String, String)>, errors::ConnectorError> {
        let timestamp = OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
        let auth: fiserv::FiservAuthType =
            fiserv::FiservAuthType::try_from(&req.connector_auth_type)?;
        let api_key = auth.api_key.clone();

        let fiserv_req = self
            .get_request_body(req)?
            .ok_or(errors::ConnectorError::RequestEncodingFailed)?;
        let client_request_id = Uuid::new_v4().to_string();
        let hmac = self
            .generate_authorization_signature(auth, &client_request_id, &fiserv_req, timestamp)
            .change_context(errors::ConnectorError::RequestEncodingFailed)?;
        let headers = vec![
            (
                headers::CONTENT_TYPE.to_string(),
                types::PaymentsAuthorizeType::get_content_type(self).to_string(),
            ),
            ("Client-Request-Id".to_string(), client_request_id),
            ("Auth-Token-Type".to_string(), "HMAC".to_string()),
            ("Api-Key".to_string(), api_key),
            ("Timestamp".to_string(), timestamp.to_string()),
            ("Authorization".to_string(), hmac),
        ];
        Ok(headers)
    }

    fn get_content_type(&self) -> &'static str {
        "application/json"
    }

    fn get_request_body(
        &self,
        req: &types::PaymentsCaptureRouterData,
    ) -> CustomResult<Option<String>, errors::ConnectorError> {
        let connector_req = fiserv::FiservCaptureRequest::try_from(req)?;
        let fiserv_req =
            utils::Encode::<fiserv::FiservCaptureRequest>::encode_to_string_of_json(&connector_req)
                .change_context(errors::ConnectorError::RequestEncodingFailed)?;
        Ok(Some(fiserv_req))
    }

    fn build_request(
        &self,
        req: &types::PaymentsCaptureRouterData,
        connectors: &settings::Connectors,
    ) -> CustomResult<Option<services::Request>, errors::ConnectorError> {
        let request = Some(
            services::RequestBuilder::new()
                .method(services::Method::Post)
                .url(&types::PaymentsCaptureType::get_url(self, req, connectors)?)
                .headers(types::PaymentsCaptureType::get_headers(
                    self, req, connectors,
                )?)
                .body(types::PaymentsCaptureType::get_request_body(self, req)?)
                .build(),
        );
        Ok(request)
    }

    fn handle_response(
        &self,
        data: &types::PaymentsCaptureRouterData,
        res: types::Response,
    ) -> CustomResult<types::PaymentsCaptureRouterData, errors::ConnectorError> {
        let response: fiserv::FiservPaymentsResponse = res
            .response
            .parse_struct("Fiserv Payment Response")
            .change_context(errors::ConnectorError::ResponseDeserializationFailed)?;
        types::ResponseRouterData {
            response,
            data: data.clone(),
            http_code: res.status_code,
        }
        .try_into()
        .change_context(errors::ConnectorError::ResponseHandlingFailed)
    }

    fn get_url(
        &self,
        _req: &types::PaymentsCaptureRouterData,
        connectors: &settings::Connectors,
    ) -> CustomResult<String, errors::ConnectorError> {
        Ok(format!(
            "{}ch/payments/v1/charges",
            connectors.fiserv.base_url
        ))
    }

    fn get_error_response(
        &self,
        res: types::Response,
    ) -> CustomResult<types::ErrorResponse, errors::ConnectorError> {
        let response: fiserv::ErrorResponse = res
            .response
            .parse_struct("Fiserv ErrorResponse")
            .change_context(errors::ConnectorError::ResponseDeserializationFailed)?;

        let fiserv::ErrorResponse { error, details } = response;

        let message = match (error, details) {
            (Some(err), _) => err
                .iter()
                .map(|v| v.message.clone())
                .collect::<Vec<String>>()
                .join(""),
            (None, Some(err_details)) => err_details
                .iter()
                .map(|v| v.message.clone())
                .collect::<Vec<String>>()
                .join(""),
            (None, None) => consts::NO_ERROR_MESSAGE.to_string(),
        };

        Ok(types::ErrorResponse {
            status_code: res.status_code,
            code: consts::NO_ERROR_CODE.to_string(),
            message,
            reason: None,
        })
    }
}

impl api::PaymentSession for Fiserv {}

#[allow(dead_code)]
impl
    services::ConnectorIntegration<
        api::Session,
        types::PaymentsSessionData,
        types::PaymentsResponseData,
    > for Fiserv
{
}

impl api::PaymentAuthorize for Fiserv {}

impl
    services::ConnectorIntegration<
        api::Authorize,
        types::PaymentsAuthorizeData,
        types::PaymentsResponseData,
    > for Fiserv
{
    fn get_headers(
        &self,
        req: &types::PaymentsAuthorizeRouterData,
        _connectors: &settings::Connectors,
    ) -> CustomResult<Vec<(String, String)>, errors::ConnectorError> {
        let timestamp = OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
        let auth: fiserv::FiservAuthType =
            fiserv::FiservAuthType::try_from(&req.connector_auth_type)?;
        let api_key = auth.api_key.clone();

        let fiserv_req = self
            .get_request_body(req)?
            .ok_or(errors::ConnectorError::RequestEncodingFailed)?;
        let client_request_id = Uuid::new_v4().to_string();
        let hmac = self
            .generate_authorization_signature(auth, &client_request_id, &fiserv_req, timestamp)
            .change_context(errors::ConnectorError::RequestEncodingFailed)?;
        let headers = vec![
            (
                headers::CONTENT_TYPE.to_string(),
                types::PaymentsAuthorizeType::get_content_type(self).to_string(),
            ),
            ("Client-Request-Id".to_string(), client_request_id),
            ("Auth-Token-Type".to_string(), "HMAC".to_string()),
            ("Api-Key".to_string(), api_key),
            ("Timestamp".to_string(), timestamp.to_string()),
            ("Authorization".to_string(), hmac),
        ];
        Ok(headers)
    }

    fn get_content_type(&self) -> &'static str {
        "application/json"
    }

    fn get_url(
        &self,
        _req: &types::PaymentsAuthorizeRouterData,
        connectors: &settings::Connectors,
    ) -> CustomResult<String, errors::ConnectorError> {
        Ok(format!(
            "{}ch/payments/v1/charges",
            connectors.fiserv.base_url
        ))
    }

    fn get_request_body(
        &self,
        req: &types::PaymentsAuthorizeRouterData,
    ) -> CustomResult<Option<String>, errors::ConnectorError> {
        let connector_req = fiserv::FiservPaymentsRequest::try_from(req)?;
        let fiserv_req = utils::Encode::<fiserv::FiservPaymentsRequest>::encode_to_string_of_json(
            &connector_req,
        )
        .change_context(errors::ConnectorError::RequestEncodingFailed)?;
        println!("autzzzzz{:?}", fiserv_req);
        Ok(Some(fiserv_req))
    }

    fn build_request(
        &self,
        req: &types::PaymentsAuthorizeRouterData,
        connectors: &settings::Connectors,
    ) -> CustomResult<Option<services::Request>, errors::ConnectorError> {
        let request = Some(
            services::RequestBuilder::new()
                .method(services::Method::Post)
                .url(&types::PaymentsAuthorizeType::get_url(
                    self, req, connectors,
                )?)
                .headers(types::PaymentsAuthorizeType::get_headers(
                    self, req, connectors,
                )?)
                .body(types::PaymentsAuthorizeType::get_request_body(self, req)?)
                .build(),
        );

        Ok(request)
    }

    fn handle_response(
        &self,
        data: &types::PaymentsAuthorizeRouterData,
        res: types::Response,
    ) -> CustomResult<types::PaymentsAuthorizeRouterData, errors::ConnectorError> {
        let response: fiserv::FiservPaymentsResponse = res
            .response
            .parse_struct("Fiserv PaymentResponse")
            .change_context(errors::ConnectorError::ResponseDeserializationFailed)?;
        types::ResponseRouterData {
            response,
            data: data.clone(),
            http_code: res.status_code,
        }
        .try_into()
        .change_context(errors::ConnectorError::ResponseHandlingFailed)
    }

    fn get_error_response(
        &self,
        res: types::Response,
    ) -> CustomResult<types::ErrorResponse, errors::ConnectorError> {
        let response: fiserv::ErrorResponse = res
            .response
            .parse_struct("Fiserv ErrorResponse")
            .change_context(errors::ConnectorError::ResponseDeserializationFailed)?;

        let fiserv::ErrorResponse { error, details } = response;

        let message = match (error, details) {
            (Some(err), _) => err
                .iter()
                .map(|v| v.message.clone())
                .collect::<Vec<String>>()
                .join(""),
            (None, Some(err_details)) => err_details
                .iter()
                .map(|v| v.message.clone())
                .collect::<Vec<String>>()
                .join(""),
            (None, None) => consts::NO_ERROR_MESSAGE.to_string(),
        };
        Ok(types::ErrorResponse {
            status_code: res.status_code,
            code: consts::NO_ERROR_CODE.to_string(),
            message,
            reason: None,
        })
    }
}

impl api::Refund for Fiserv {}
impl api::RefundExecute for Fiserv {}
impl api::RefundSync for Fiserv {}

#[allow(dead_code)]
impl services::ConnectorIntegration<api::Execute, types::RefundsData, types::RefundsResponseData>
    for Fiserv
{
    fn get_headers(
        &self,
        req: &types::RefundsRouterData<api::Execute>,
        _connectors: &settings::Connectors,
    ) -> CustomResult<Vec<(String, String)>, errors::ConnectorError> {
        let timestamp = OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
        let auth: fiserv::FiservAuthType =
            fiserv::FiservAuthType::try_from(&req.connector_auth_type)?;
        let api_key = auth.api_key.clone();

        let fiserv_req = self
            .get_request_body(req)?
            .ok_or(errors::ConnectorError::RequestEncodingFailed)?;
        let client_request_id = Uuid::new_v4().to_string();
        let hmac = self
            .generate_authorization_signature(auth, &client_request_id, &fiserv_req, timestamp)
            .change_context(errors::ConnectorError::RequestEncodingFailed)?;
        let headers = vec![
            (
                headers::CONTENT_TYPE.to_string(),
                types::PaymentsAuthorizeType::get_content_type(self).to_string(),
            ),
            ("Client-Request-Id".to_string(), client_request_id),
            ("Auth-Token-Type".to_string(), "HMAC".to_string()),
            ("Api-Key".to_string(), api_key),
            ("Timestamp".to_string(), timestamp.to_string()),
            ("Authorization".to_string(), hmac),
        ];
        Ok(headers)
    }
    fn get_content_type(&self) -> &'static str {
        "application/json"
    }
    fn get_url(
        &self,
        _req: &types::RefundsRouterData<api::Execute>,
        connectors: &settings::Connectors,
    ) -> CustomResult<String, errors::ConnectorError> {
        Ok(format!(
            "{}ch/payments/v1/refunds",
            connectors.fiserv.base_url
        ))
    }
    fn get_request_body(
        &self,
        req: &types::RefundsRouterData<api::Execute>,
    ) -> CustomResult<Option<String>, errors::ConnectorError> {
        let connector_req = fiserv::FiservRefundRequest::try_from(req)?;
        let fiserv_req =
            utils::Encode::<fiserv::FiservRefundRequest>::encode_to_string_of_json(&connector_req)
                .change_context(errors::ConnectorError::RequestEncodingFailed)?;
        println!("@@@@@{:?}", fiserv_req);
        Ok(Some(fiserv_req))
    }
    fn build_request(
        &self,
        req: &types::RefundsRouterData<api::Execute>,
        connectors: &settings::Connectors,
    ) -> CustomResult<Option<services::Request>, errors::ConnectorError> {
        let request = services::RequestBuilder::new()
            .method(services::Method::Post)
            .url(&types::RefundExecuteType::get_url(self, req, connectors)?)
            .headers(types::RefundExecuteType::get_headers(
                self, req, connectors,
            )?)
            .body(types::RefundExecuteType::get_request_body(self, req)?)
            .build();
        Ok(Some(request))
    }

    fn handle_response(
        &self,
        data: &types::RefundsRouterData<api::Execute>,
        res: types::Response,
    ) -> CustomResult<types::RefundsRouterData<api::Execute>, errors::ConnectorError> {
        logger::debug!(target: "router::connector::fiserv", response=?res);
        let response: fiserv::RefundResponse =
            res.response
                .parse_struct("fiserv RefundResponse")
                .change_context(errors::ConnectorError::RequestEncodingFailed)?;
        types::ResponseRouterData {
            response,
            data: data.clone(),
            http_code: res.status_code,
        }
        .try_into()
        .change_context(errors::ConnectorError::ResponseHandlingFailed)
    }
    fn get_error_response(
        &self,
        res: types::Response,
    ) -> CustomResult<types::ErrorResponse, errors::ConnectorError> {
        self.build_error_response(res)
    }
}

#[allow(dead_code)]
impl services::ConnectorIntegration<api::RSync, types::RefundsData, types::RefundsResponseData>
    for Fiserv
{
    fn get_headers(
        &self,
        req: &types::RefundSyncRouterData,
        _connectors: &settings::Connectors,
    ) -> CustomResult<Vec<(String, String)>, errors::ConnectorError> {
        let timestamp = OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
        let auth: fiserv::FiservAuthType =
            fiserv::FiservAuthType::try_from(&req.connector_auth_type)?;
        let api_key = auth.api_key.clone();

        let fiserv_req = self
            .get_request_body(req)?
            .ok_or(errors::ConnectorError::RequestEncodingFailed)?;
        let client_request_id = Uuid::new_v4().to_string();
        let hmac = self
            .generate_authorization_signature(auth, &client_request_id, &fiserv_req, timestamp)
            .change_context(errors::ConnectorError::RequestEncodingFailed)?;
        let headers = vec![
            (
                headers::CONTENT_TYPE.to_string(),
                types::PaymentsAuthorizeType::get_content_type(self).to_string(),
            ),
            ("Client-Request-Id".to_string(), client_request_id),
            ("Auth-Token-Type".to_string(), "HMAC".to_string()),
            ("Api-Key".to_string(), api_key),
            ("Timestamp".to_string(), timestamp.to_string()),
            ("Authorization".to_string(), hmac),
        ];
        Ok(headers)
    }

    fn get_content_type(&self) -> &'static str {
        "application/json"
    }

    fn get_url(
        &self,
        _req: &types::RefundSyncRouterData,
        connectors: &settings::Connectors,
    ) -> CustomResult<String, errors::ConnectorError> {
        Ok(format!(
            "{}ch/payments/v1/transaction-inquiry",
            connectors.fiserv.base_url
        ))
    }

    fn get_request_body(
        &self,
        req: &types::RefundSyncRouterData,
    ) -> CustomResult<Option<String>, errors::ConnectorError> {
        let connector_req = fiserv::FiservSyncRequest::try_from(req)?;
        let fiserv_req =
            utils::Encode::<fiserv::FiservSyncRequest>::encode_to_string_of_json(&connector_req)
                .change_context(errors::ConnectorError::RequestEncodingFailed)?;
        println!("ahhhhhhhhhh{:?}", fiserv_req);
        Ok(Some(fiserv_req))
    }

    fn build_request(
        &self,
        req: &types::RefundSyncRouterData,
        connectors: &settings::Connectors,
    ) -> CustomResult<Option<services::Request>, errors::ConnectorError> {
        let request = Some(
            services::RequestBuilder::new()
                .method(services::Method::Post)
                .url(&types::RefundSyncType::get_url(self, req, connectors)?)
                .headers(types::RefundSyncType::get_headers(self, req, connectors)?)
                .body(types::RefundSyncType::get_request_body(self, req)?)
                .build(),
        );
        Ok(request)
    }

    fn handle_response(
        &self,
        data: &types::RefundSyncRouterData,
        res: types::Response,
    ) -> CustomResult<types::RefundSyncRouterData, errors::ConnectorError> {
        logger::debug!(target: "router::connector::fiserv", response=?res);

        let response: Vec<fiserv::RefundResponse> = res
            .response
            .parse_struct("Fiserv Refund Response")
            .change_context(errors::ConnectorError::ResponseDeserializationFailed)?;
        types::ResponseRouterData {
            // response:response.first().get_required_value("gatewayResponse").change_context(errors::ConnectorError::ResponseDeserializationFailed).try_into(),
            response: response[0].clone(),
            data: data.clone(),
            http_code: res.status_code,
        }
        .try_into()
        .change_context(errors::ConnectorError::ResponseHandlingFailed)
    }

    fn get_error_response(
        &self,
        res: types::Response,
    ) -> CustomResult<types::ErrorResponse, errors::ConnectorError> {
        self.build_error_response(res)
    }
}

#[async_trait::async_trait]
impl api::IncomingWebhook for Fiserv {
    fn get_webhook_object_reference_id(
        &self,
        _body: &[u8],
    ) -> CustomResult<String, errors::ConnectorError> {
        Err(errors::ConnectorError::NotImplemented("fiserv".to_string()).into())
    }

    fn get_webhook_event_type(
        &self,
        _body: &[u8],
    ) -> CustomResult<api::IncomingWebhookEvent, errors::ConnectorError> {
        Err(errors::ConnectorError::NotImplemented("fiserv".to_string()).into())
    }

    fn get_webhook_resource_object(
        &self,
        _body: &[u8],
    ) -> CustomResult<serde_json::Value, errors::ConnectorError> {
        Err(errors::ConnectorError::NotImplemented("fiserv".to_string()).into())
    }
}

impl services::ConnectorRedirectResponse for Fiserv {
    fn get_flow_type(
        &self,
        _query_params: &str,
    ) -> CustomResult<payments::CallConnectorAction, errors::ConnectorError> {
        Ok(payments::CallConnectorAction::Trigger)
    }
}

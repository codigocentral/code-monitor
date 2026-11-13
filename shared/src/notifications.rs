//! Notification system for alerts
//!
//! Supports multiple channels: Webhook, Slack, Discord, Email

use crate::alerts::Alert;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Notification channel trait
#[async_trait::async_trait]
pub trait NotificationChannel: Send + Sync {
    async fn send(&self, alert: &Alert) -> Result<()>;
    fn name(&self) -> &str;
}

#[cfg(test)]
mod campaign_transport_units {
    use super::*;
    use crate::alerts::{AlertSeverity, AlertType};
    use chrono::TimeZone;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

    #[derive(Clone)]
    struct CaptureSubscriber(Arc<Mutex<Vec<String>>>);
    impl tracing::Subscriber for CaptureSubscriber {
        fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            struct Visitor(Vec<String>);
            impl tracing::field::Visit for Visitor {
                fn record_debug(
                    &mut self,
                    field: &tracing::field::Field,
                    value: &dyn std::fmt::Debug,
                ) {
                    self.0.push(format!("{}={value:?}", field.name()));
                }
            }
            let mut visitor = Visitor(Vec::new());
            event.record(&mut visitor);
            self.0.lock().unwrap().extend(visitor.0);
        }
        fn enter(&self, _span: &tracing::span::Id) {}
        fn exit(&self, _span: &tracing::span::Id) {}
    }

    struct Poster {
        requests: Mutex<Vec<(String, serde_json::Value)>>,
        fails: bool,
        status: u16,
    }
    impl Poster {
        fn new(fails: bool, status: u16) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                fails,
                status,
            }
        }
        fn request(&self) -> (String, serde_json::Value) {
            let requests = self.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            requests[0].clone()
        }
    }
    #[async_trait::async_trait]
    impl JsonPoster for Poster {
        async fn post(
            &self,
            _client: &reqwest::Client,
            url: &str,
            payload: &serde_json::Value,
        ) -> reqwest::Result<reqwest::Response> {
            self.requests
                .lock()
                .unwrap()
                .push((url.into(), payload.clone()));
            if self.fails {
                Err(reqwest::Client::new().get("\0").build().unwrap_err())
            } else {
                Ok(http::Response::builder()
                    .status(self.status)
                    .body("unit response".to_string())
                    .unwrap()
                    .into())
            }
        }
    }
    fn alert() -> Alert {
        let mut alert = Alert::new(
            AlertType::CpuHigh,
            AlertSeverity::Warning,
            "srv-unit".into(),
            "Unit Server".into(),
            "CPU high".into(),
            Some(95.0),
            Some(80.0),
        );
        alert.id = uuid::Uuid::nil();
        alert.triggered_at = chrono::Utc.timestamp_opt(10, 0).unwrap();
        alert
    }
    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.timestamp_opt(20, 0).unwrap()
    }

    #[test]
    fn request_builder_preserves_method_payload_content_type_and_timeout() {
        let payload = json!({"event":"unit", "optional":null});
        let request = json_request(
            &reqwest::Client::new(),
            "https://unit.invalid/hook",
            &payload,
        )
        .build()
        .unwrap();
        assert_eq!(request.method(), reqwest::Method::POST);
        assert_eq!(request.url().as_str(), "https://unit.invalid/hook");
        assert_eq!(request.timeout(), Some(&std::time::Duration::from_secs(10)));
        assert_eq!(request.headers()["content-type"], "application/json");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(
                request.body().unwrap().as_bytes().unwrap()
            )
            .unwrap(),
            payload
        );
    }

    #[tokio::test]
    async fn webhook_sends_exact_event_and_uses_supplied_clock() {
        let channel = WebhookChannel::new("unit".into(), "https://unit.invalid/hook".into());
        let poster = Poster::new(false, 200);
        channel.send_using(&alert(), &poster, now).await.unwrap();
        let (url, payload) = poster.request();
        assert_eq!(url, "https://unit.invalid/hook");
        assert_eq!(
            payload,
            json!({
                "event":"alert", "alert_id":"00000000-0000-0000-0000-000000000000",
                "alert_type":"CPU_HIGH", "severity":"warning", "server_id":"srv-unit", "server_name":"Unit Server",
                "message":"CPU high", "value":95.0, "threshold":80.0,
                "triggered_at":"1970-01-01T00:00:10+00:00", "timestamp":"1970-01-01T00:00:20+00:00"
            })
        );
    }

    #[tokio::test]
    async fn slack_and_discord_render_all_severities_and_keep_http_status_behavior() {
        for (severity, slack_color, discord_color, label) in [
            (AlertSeverity::Info, "#36a64f", 0x36a64f, "info"),
            (AlertSeverity::Warning, "#ff9900", 0xff9900, "warning"),
            (AlertSeverity::Critical, "#ff0000", 0xff0000, "critical"),
        ] {
            let mut alert = alert();
            alert.severity = severity;
            // Existing behavior accepts HTTP responses without error_for_status.
            let slack = Poster::new(false, 500);
            SlackChannel::new("slack".into(), "https://unit.invalid/slack".into())
                .send_using(&alert, &slack, now)
                .await
                .unwrap();
            let (url, payload) = slack.request();
            assert_eq!(url, "https://unit.invalid/slack");
            assert_eq!(payload["text"], "🚨 Alert: CPU high");
            let attachment = &payload["attachments"][0];
            assert_eq!(attachment["color"], slack_color);
            assert_eq!(attachment["footer"], "Code Monitor");
            assert_eq!(attachment["ts"], 20);
            assert_eq!(
                attachment["fields"],
                json!([
                    {"title":"Server","value":"Unit Server","short":true},
                    {"title":"Type","value":"CPU_HIGH","short":true},
                    {"title":"Severity","value":label,"short":true},
                    {"title":"Triggered At","value":"1970-01-01 00:00:10 UTC","short":true}
                ])
            );
            let discord = Poster::new(false, 503);
            DiscordChannel::new("discord".into(), "https://unit.invalid/discord".into())
                .send_using(&alert, &discord, now)
                .await
                .unwrap();
            let (url, payload) = discord.request();
            assert_eq!(url, "https://unit.invalid/discord");
            assert_eq!(payload["content"], "🚨 **Alert**: CPU high");
            let embed = &payload["embeds"][0];
            assert_eq!(embed["title"], "CPU_HIGH Alert");
            assert_eq!(embed["color"], discord_color);
            assert_eq!(embed["timestamp"], "1970-01-01T00:00:20+00:00");
            assert_eq!(
                embed["fields"],
                json!([
                    {"name":"Server","value":"Unit Server","inline":true},
                    {"name":"Type","value":"CPU_HIGH","inline":true},
                    {"name":"Severity","value":label,"inline":true},
                    {"name":"Triggered At","value":"1970-01-01 00:00:10 UTC","inline":true}
                ])
            );
        }
    }

    #[tokio::test]
    async fn each_channel_preserves_transport_error_context() {
        let alert = alert();
        let failure = Poster::new(true, 0);
        let webhook = WebhookChannel::new("webhook".into(), "https://unit.invalid/webhook".into());
        assert!(webhook
            .send_using(&alert, &failure, now)
            .await
            .unwrap_err()
            .to_string()
            .starts_with("Webhook request failed:"));
        let slack = SlackChannel::new("slack".into(), "https://unit.invalid/slack".into());
        assert!(slack
            .send_using(&alert, &failure, now)
            .await
            .unwrap_err()
            .to_string()
            .starts_with("Slack request failed:"));
        let discord = DiscordChannel::new("discord".into(), "https://unit.invalid/discord".into());
        assert!(discord
            .send_using(&alert, &failure, now)
            .await
            .unwrap_err()
            .to_string()
            .starts_with("Discord request failed:"));
        assert_eq!(failure.requests.lock().unwrap().len(), 3);
    }

    struct SuccessfulChannel(Arc<AtomicUsize>);
    #[async_trait::async_trait]
    impl NotificationChannel for SuccessfulChannel {
        async fn send(&self, _alert: &Alert) -> Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn name(&self) -> &str {
            "successful-unit"
        }
    }
    #[tokio::test]
    async fn dispatcher_delivers_to_each_successful_channel_in_order() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut dispatcher = NotificationDispatcher::new();
        dispatcher.add_channel(Box::new(SuccessfulChannel(calls.clone())));
        dispatcher.add_channel(Box::new(SuccessfulChannel(calls.clone())));
        let results = dispatcher.dispatch(&alert()).await;
        assert_eq!(results.len(), 2);
        assert!(results.into_iter().all(|r| r.is_ok()));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    struct FailingChannel;
    #[async_trait::async_trait]
    impl NotificationChannel for FailingChannel {
        async fn send(&self, _alert: &Alert) -> Result<()> {
            anyhow::bail!("unit channel failure")
        }
        fn name(&self) -> &str {
            "failing-unit"
        }
    }
    #[tokio::test]
    async fn dispatcher_reports_failed_channel_to_a_log_subscriber_double() {
        let logs = Arc::new(Mutex::new(Vec::new()));
        let _guard = tracing::subscriber::set_default(CaptureSubscriber(logs.clone()));
        let mut dispatcher = NotificationDispatcher::new();
        dispatcher.add_channel(Box::new(FailingChannel));
        let results = dispatcher.dispatch(&alert()).await;
        assert!(results[0].is_err());
        let messages = logs.lock().unwrap().join("\n");
        assert!(messages.contains("failing-unit"));
        assert!(messages.contains("unit channel failure"));
    }
}

// The transport boundary stays internal; public channel fields/API are unchanged.
#[async_trait::async_trait]
trait JsonPoster: Send + Sync {
    async fn post(
        &self,
        client: &reqwest::Client,
        url: &str,
        payload: &serde_json::Value,
    ) -> reqwest::Result<reqwest::Response>;
}

struct ReqwestJsonPoster;

fn json_request(
    client: &reqwest::Client,
    url: &str,
    payload: &serde_json::Value,
) -> reqwest::RequestBuilder {
    client
        .post(url)
        .json(payload)
        .timeout(std::time::Duration::from_secs(10))
}

#[async_trait::async_trait]
impl JsonPoster for ReqwestJsonPoster {
    async fn post(
        &self,
        client: &reqwest::Client,
        url: &str,
        payload: &serde_json::Value,
    ) -> reqwest::Result<reqwest::Response> {
        json_request(client, url, payload).send().await
    }
}

/// Webhook notification channel
#[derive(Debug, Clone)]
pub struct WebhookChannel {
    pub name: String,
    pub url: String,
    pub client: reqwest::Client,
}

impl WebhookChannel {
    pub fn new(name: String, url: String) -> Self {
        Self {
            name,
            url,
            client: reqwest::Client::new(),
        }
    }
}

impl WebhookChannel {
    async fn send_using(
        &self,
        alert: &Alert,
        poster: &dyn JsonPoster,
        clock: impl Fn() -> chrono::DateTime<chrono::Utc> + Send,
    ) -> Result<()> {
        let payload = json!({
            "event": "alert",
            "alert_id": alert.id.to_string(),
            "alert_type": alert.alert_type.to_string(),
            "severity": alert.severity.to_string(),
            "server_id": alert.server_id,
            "server_name": alert.server_name,
            "message": alert.message,
            "value": alert.value,
            "threshold": alert.threshold,
            "triggered_at": alert.triggered_at.to_rfc3339(),
            "timestamp": clock().to_rfc3339(),
        });

        poster
            .post(&self.client, &self.url, &payload)
            .await
            .map_err(|e| anyhow::anyhow!("Webhook request failed: {}", e))?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl NotificationChannel for WebhookChannel {
    async fn send(&self, alert: &Alert) -> Result<()> {
        self.send_using(alert, &ReqwestJsonPoster, chrono::Utc::now)
            .await
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Slack notification channel
#[derive(Debug, Clone)]
pub struct SlackChannel {
    pub name: String,
    pub webhook_url: String,
    pub client: reqwest::Client,
}

impl SlackChannel {
    pub fn new(name: String, webhook_url: String) -> Self {
        Self {
            name,
            webhook_url,
            client: reqwest::Client::new(),
        }
    }

    fn severity_color(&self, severity: &crate::alerts::AlertSeverity) -> &'static str {
        match severity {
            crate::alerts::AlertSeverity::Info => "#36a64f",
            crate::alerts::AlertSeverity::Warning => "#ff9900",
            crate::alerts::AlertSeverity::Critical => "#ff0000",
        }
    }
}

impl SlackChannel {
    async fn send_using(
        &self,
        alert: &Alert,
        poster: &dyn JsonPoster,
        clock: impl Fn() -> chrono::DateTime<chrono::Utc> + Send,
    ) -> Result<()> {
        let color = self.severity_color(&alert.severity);

        let payload = json!({
            "text": format!("🚨 Alert: {}", alert.message),
            "attachments": [
                {
                    "color": color,
                    "fields": [
                        {
                            "title": "Server",
                            "value": &alert.server_name,
                            "short": true
                        },
                        {
                            "title": "Type",
                            "value": alert.alert_type.to_string(),
                            "short": true
                        },
                        {
                            "title": "Severity",
                            "value": alert.severity.to_string(),
                            "short": true
                        },
                        {
                            "title": "Triggered At",
                            "value": alert.triggered_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                            "short": true
                        }
                    ],
                    "footer": "Code Monitor",
                    "ts": clock().timestamp()
                }
            ]
        });

        poster
            .post(&self.client, &self.webhook_url, &payload)
            .await
            .map_err(|e| anyhow::anyhow!("Slack request failed: {}", e))?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl NotificationChannel for SlackChannel {
    async fn send(&self, alert: &Alert) -> Result<()> {
        self.send_using(alert, &ReqwestJsonPoster, chrono::Utc::now)
            .await
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Discord notification channel
#[derive(Debug, Clone)]
pub struct DiscordChannel {
    pub name: String,
    pub webhook_url: String,
    pub client: reqwest::Client,
}

impl DiscordChannel {
    pub fn new(name: String, webhook_url: String) -> Self {
        Self {
            name,
            webhook_url,
            client: reqwest::Client::new(),
        }
    }

    fn severity_color(&self, severity: &crate::alerts::AlertSeverity) -> u64 {
        match severity {
            crate::alerts::AlertSeverity::Info => 0x36a64f,
            crate::alerts::AlertSeverity::Warning => 0xff9900,
            crate::alerts::AlertSeverity::Critical => 0xff0000,
        }
    }
}

impl DiscordChannel {
    async fn send_using(
        &self,
        alert: &Alert,
        poster: &dyn JsonPoster,
        clock: impl Fn() -> chrono::DateTime<chrono::Utc> + Send,
    ) -> Result<()> {
        let color = self.severity_color(&alert.severity);

        let payload = json!({
            "content": format!("🚨 **Alert**: {}", alert.message),
            "embeds": [
                {
                    "title": format!("{} Alert", alert.alert_type),
                    "color": color,
                    "fields": [
                        {
                            "name": "Server",
                            "value": &alert.server_name,
                            "inline": true
                        },
                        {
                            "name": "Type",
                            "value": alert.alert_type.to_string(),
                            "inline": true
                        },
                        {
                            "name": "Severity",
                            "value": alert.severity.to_string(),
                            "inline": true
                        },
                        {
                            "name": "Triggered At",
                            "value": alert.triggered_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                            "inline": true
                        }
                    ],
                    "timestamp": clock().to_rfc3339()
                }
            ]
        });

        poster
            .post(&self.client, &self.webhook_url, &payload)
            .await
            .map_err(|e| anyhow::anyhow!("Discord request failed: {}", e))?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl NotificationChannel for DiscordChannel {
    async fn send(&self, alert: &Alert) -> Result<()> {
        self.send_using(alert, &ReqwestJsonPoster, chrono::Utc::now)
            .await
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Notification dispatcher
pub struct NotificationDispatcher {
    channels: Vec<Box<dyn NotificationChannel>>,
}

impl NotificationDispatcher {
    pub fn new() -> Self {
        Self {
            channels: Vec::new(),
        }
    }

    pub fn add_channel(&mut self, channel: Box<dyn NotificationChannel>) {
        self.channels.push(channel);
    }

    pub async fn dispatch(&self, alert: &Alert) -> Vec<Result<()>> {
        let mut results = Vec::new();

        for channel in &self.channels {
            let result = channel.send(alert).await;
            if let Err(ref e) = result {
                tracing::error!("Failed to send notification to {}: {}", channel.name(), e);
            }
            results.push(result);
        }

        results
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }
}

impl Default for NotificationDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for notification channels
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotificationConfig {
    pub webhooks: Vec<WebhookConfig>,
    pub slack: Vec<SlackConfig>,
    pub discord: Vec<DiscordConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    pub name: String,
    pub webhook_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    pub name: String,
    pub webhook_url: String,
}

impl NotificationConfig {
    pub fn build_dispatcher(&self) -> NotificationDispatcher {
        let mut dispatcher = NotificationDispatcher::new();

        for webhook in &self.webhooks {
            dispatcher.add_channel(Box::new(WebhookChannel::new(
                webhook.name.clone(),
                webhook.url.clone(),
            )));
        }

        for slack in &self.slack {
            dispatcher.add_channel(Box::new(SlackChannel::new(
                slack.name.clone(),
                slack.webhook_url.clone(),
            )));
        }

        for discord in &self.discord {
            dispatcher.add_channel(Box::new(DiscordChannel::new(
                discord.name.clone(),
                discord.webhook_url.clone(),
            )));
        }

        dispatcher
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::AlertSeverity;

    #[test]
    fn test_discord_channel_severity_color() {
        let channel = DiscordChannel::new("test".to_string(), "http://example.com".to_string());
        assert_eq!(channel.severity_color(&AlertSeverity::Info), 0x36a64f);
        assert_eq!(channel.severity_color(&AlertSeverity::Warning), 0xff9900);
        assert_eq!(channel.severity_color(&AlertSeverity::Critical), 0xff0000);
    }

    #[test]
    fn test_notification_dispatcher_empty() {
        let dispatcher = NotificationDispatcher::new();
        assert_eq!(dispatcher.channel_count(), 0);
    }

    #[test]
    fn test_notification_config_default() {
        let config = NotificationConfig::default();
        assert!(config.webhooks.is_empty());
        assert!(config.slack.is_empty());
        assert!(config.discord.is_empty());
    }

    #[test]
    fn test_notification_config_build_dispatcher() {
        let config = NotificationConfig {
            webhooks: vec![],
            slack: vec![],
            discord: vec![DiscordConfig {
                name: "test-discord".to_string(),
                webhook_url: "https://discord.com/api/webhooks/test".to_string(),
            }],
        };
        let dispatcher = config.build_dispatcher();
        assert_eq!(dispatcher.channel_count(), 1);
    }

    #[test]
    fn test_notification_config_build_dispatcher_multi_channel() {
        let config = NotificationConfig {
            webhooks: vec![WebhookConfig {
                name: "webhook-1".to_string(),
                url: "https://example.com/webhook".to_string(),
            }],
            slack: vec![SlackConfig {
                name: "slack-1".to_string(),
                webhook_url: "https://hooks.slack.com/test".to_string(),
            }],
            discord: vec![DiscordConfig {
                name: "discord-1".to_string(),
                webhook_url: "https://discord.com/api/webhooks/test".to_string(),
            }],
        };
        let dispatcher = config.build_dispatcher();
        assert_eq!(dispatcher.channel_count(), 3);
    }

    #[test]
    fn test_slack_severity_color() {
        let channel = SlackChannel::new(
            "test".to_string(),
            "https://hooks.slack.com/test".to_string(),
        );
        assert_eq!(channel.severity_color(&AlertSeverity::Info), "#36a64f");
        assert_eq!(channel.severity_color(&AlertSeverity::Warning), "#ff9900");
        assert_eq!(channel.severity_color(&AlertSeverity::Critical), "#ff0000");
    }

    #[test]
    fn test_webhook_channel_name() {
        let channel =
            WebhookChannel::new("my-webhook".to_string(), "https://example.com".to_string());
        assert_eq!(channel.name(), "my-webhook");
    }

    #[test]
    fn test_slack_channel_name() {
        let channel = SlackChannel::new(
            "my-slack".to_string(),
            "https://hooks.slack.com".to_string(),
        );
        assert_eq!(channel.name(), "my-slack");
    }

    #[test]
    fn test_discord_channel_name() {
        let channel =
            DiscordChannel::new("my-discord".to_string(), "https://discord.com".to_string());
        assert_eq!(channel.name(), "my-discord");
    }

    #[tokio::test]
    async fn test_notification_dispatcher_empty_dispatch() {
        let dispatcher = NotificationDispatcher::new();
        let alert = crate::alerts::Alert::new(
            crate::alerts::AlertType::CpuHigh,
            crate::alerts::AlertSeverity::Warning,
            "srv1".to_string(),
            "Server 1".to_string(),
            "CPU high".to_string(),
            Some(95.0),
            Some(80.0),
        );
        let results = dispatcher.dispatch(&alert).await;
        assert!(results.is_empty());
    }

    #[test]
    fn test_notification_dispatcher_default() {
        let dispatcher = NotificationDispatcher::default();
        assert_eq!(dispatcher.channel_count(), 0);
    }

    struct ErrorChannel {
        name: String,
    }

    #[async_trait::async_trait]
    impl NotificationChannel for ErrorChannel {
        async fn send(&self, _alert: &crate::alerts::Alert) -> anyhow::Result<()> {
            Err(anyhow::anyhow!("test error"))
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    #[tokio::test]
    async fn test_notification_dispatcher_dispatch_error() {
        let mut dispatcher = NotificationDispatcher::new();
        dispatcher.add_channel(Box::new(ErrorChannel {
            name: "error-channel".to_string(),
        }));

        let alert = crate::alerts::Alert::new(
            crate::alerts::AlertType::CpuHigh,
            crate::alerts::AlertSeverity::Warning,
            "srv1".to_string(),
            "Server 1".to_string(),
            "CPU high".to_string(),
            Some(95.0),
            Some(80.0),
        );

        let results = dispatcher.dispatch(&alert).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }

    #[tokio::test]
    async fn test_webhook_channel_rejects_invalid_url() {
        let channel = WebhookChannel::new("test".to_string(), "\0".to_string());
        let alert = crate::alerts::Alert::new(
            crate::alerts::AlertType::CpuHigh,
            crate::alerts::AlertSeverity::Warning,
            "srv1".to_string(),
            "Server 1".to_string(),
            "CPU high".to_string(),
            Some(95.0),
            Some(80.0),
        );
        let result = channel.send(&alert).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_slack_channel_rejects_invalid_url() {
        let channel = SlackChannel::new("test".to_string(), "\0".to_string());
        let alert = crate::alerts::Alert::new(
            crate::alerts::AlertType::CpuHigh,
            crate::alerts::AlertSeverity::Warning,
            "srv1".to_string(),
            "Server 1".to_string(),
            "CPU high".to_string(),
            Some(95.0),
            Some(80.0),
        );
        let result = channel.send(&alert).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_discord_channel_rejects_invalid_url() {
        let channel = DiscordChannel::new("test".to_string(), "\0".to_string());
        let alert = crate::alerts::Alert::new(
            crate::alerts::AlertType::CpuHigh,
            crate::alerts::AlertSeverity::Warning,
            "srv1".to_string(),
            "Server 1".to_string(),
            "CPU high".to_string(),
            Some(95.0),
            Some(80.0),
        );
        let result = channel.send(&alert).await;
        assert!(result.is_err());
    }
}

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

#[async_trait::async_trait]
impl NotificationChannel for WebhookChannel {
    async fn send(&self, alert: &Alert) -> Result<()> {
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
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        self.client
            .post(&self.url)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Webhook request failed: {}", e))?;

        Ok(())
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

#[async_trait::async_trait]
impl NotificationChannel for SlackChannel {
    async fn send(&self, alert: &Alert) -> Result<()> {
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
                    "ts": chrono::Utc::now().timestamp()
                }
            ]
        });

        self.client
            .post(&self.webhook_url)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Slack request failed: {}", e))?;

        Ok(())
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

#[async_trait::async_trait]
impl NotificationChannel for DiscordChannel {
    async fn send(&self, alert: &Alert) -> Result<()> {
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
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }
            ]
        });

        self.client
            .post(&self.webhook_url)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Discord request failed: {}", e))?;

        Ok(())
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
        let channel = SlackChannel::new("test".to_string(), "https://hooks.slack.com/test".to_string());
        assert_eq!(channel.severity_color(&AlertSeverity::Info), "#36a64f");
        assert_eq!(channel.severity_color(&AlertSeverity::Warning), "#ff9900");
        assert_eq!(channel.severity_color(&AlertSeverity::Critical), "#ff0000");
    }

    #[test]
    fn test_webhook_channel_name() {
        let channel = WebhookChannel::new("my-webhook".to_string(), "https://example.com".to_string());
        assert_eq!(channel.name(), "my-webhook");
    }

    #[test]
    fn test_slack_channel_name() {
        let channel = SlackChannel::new("my-slack".to_string(), "https://hooks.slack.com".to_string());
        assert_eq!(channel.name(), "my-slack");
    }

    #[test]
    fn test_discord_channel_name() {
        let channel = DiscordChannel::new("my-discord".to_string(), "https://discord.com".to_string());
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
    async fn test_webhook_channel_send_error() {
        let channel = WebhookChannel::new(
            "test".to_string(),
            "http://127.0.0.1:1/webhook".to_string(),
        );
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
    async fn test_slack_channel_send_error() {
        let channel = SlackChannel::new(
            "test".to_string(),
            "http://127.0.0.1:1/slack".to_string(),
        );
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
    async fn test_discord_channel_send_error() {
        let channel = DiscordChannel::new(
            "test".to_string(),
            "http://127.0.0.1:1/discord".to_string(),
        );
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

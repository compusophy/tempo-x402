use serde::{Deserialize, Serialize};
use tracing::info;

/// Trait for pricing strategies that can dynamically adjust endpoint prices.
pub trait PricingStrategy {
    /// Calculates the next price for an endpoint based on its current price and performance metrics.
    fn calculate_price(&self, slug: &str, current_price_usd: f64, metrics: &EndpointMetrics) -> f64;
}

/// Aggregated performance metrics for an endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EndpointMetrics {
    /// Percentage of successful requests (0.0 to 1.0)
    pub success_rate: f64,
    /// Average latency in milliseconds
    pub avg_latency_ms: f64,
    /// Resource usage intensity (0.0 to 1.0)
    pub resource_intensity: f64,
    /// Number of requests in the measurement window
    pub request_count: u64,
    /// Number of successful payments
    pub payment_count: u64,
}

/// Dynamic pricing implementation that adjusts prices based on fitness-like metrics.
pub struct DynamicPricing {
    /// Minimum allowed price in USD
    pub min_price: f64,
    /// Maximum allowed price in USD
    pub max_price: f64,
    /// Target success rate (e.g., 0.99)
    pub target_success_rate: f64,
    /// Latency threshold in milliseconds (e.g., 500ms)
    pub latency_threshold_ms: f64,
}

impl Default for DynamicPricing {
    fn default() -> Self {
        Self {
            min_price: 0.0001,
            max_price: 1.0,
            target_success_rate: 0.98,
            latency_threshold_ms: 300.0,
        }
    }
}

impl PricingStrategy for DynamicPricing {
    fn calculate_price(&self, slug: &str, current_price_usd: f64, metrics: &EndpointMetrics) -> f64 {
        let mut multiplier = 1.0;

        // 1. Success Rate Adjustment
        if metrics.request_count > 0 {
            if metrics.success_rate < self.target_success_rate {
                let penalty = (self.target_success_rate - metrics.success_rate) * 2.0;
                multiplier *= (1.0 - penalty).max(0.5);
                info!(slug = %slug, success_rate = %metrics.success_rate, "Adjusting price multiplier due to low success rate");
            } else if metrics.success_rate > 0.995 && metrics.request_count > 10 {
                multiplier *= 1.02;
            }
        }

        // 2. Latency Adjustment
        if metrics.avg_latency_ms > 0.0 {
            if metrics.avg_latency_ms < self.latency_threshold_ms / 2.0 {
                multiplier *= 1.05;
            } else if metrics.avg_latency_ms > self.latency_threshold_ms {
                multiplier *= 0.90;
            }
        }

        // 3. Resource Usage Adjustment
        if metrics.resource_intensity > 0.8 {
            multiplier *= 1.25;
        } else if metrics.resource_intensity < 0.1 && metrics.request_count > 0 {
            multiplier *= 0.95;
        }

        // 4. Demand Adjustment
        if metrics.request_count > 0 {
            let payment_conversion = metrics.payment_count as f64 / metrics.request_count as f64;
            if payment_conversion > 0.5 {
                multiplier *= 1.1;
            }
        }

        let mut new_price = current_price_usd * multiplier;

        if new_price < self.min_price {
            new_price = self.min_price;
        } else if new_price > self.max_price {
            new_price = self.max_price;
        }

        info!(
            slug = %slug,
            old_price = %current_price_usd,
            new_price = %new_price,
            multiplier = %multiplier,
            "Calculated dynamic price"
        );

        new_price
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dynamic_pricing_success_rate() {
        let strategy = DynamicPricing::default();
        let metrics = EndpointMetrics {
            success_rate: 0.5,
            request_count: 10,
            ..Default::default()
        };
        
        let price = strategy.calculate_price("test", 0.01, &metrics);
        assert!(price < 0.01);
    }

    #[test]
    fn test_dynamic_pricing_high_resource() {
        let strategy = DynamicPricing::default();
        let metrics = EndpointMetrics {
            success_rate: 1.0,
            resource_intensity: 0.9,
            request_count: 1,
            ..Default::default()
        };
        
        let price = strategy.calculate_price("test", 0.01, &metrics);
        assert!(price > 0.01);
    }
}

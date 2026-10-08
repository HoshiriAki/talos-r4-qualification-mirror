//! Frozen R4 extension facets.
//!
//! These identifiers reserve adapter seams only. They are not runtime
//! dependencies, do not select products by name in business code, and do not
//! weaken the capability requirements declared by the stable Interconnect SPI.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExtensionFacet {
    KafkaCompatibleLog,
    Pulsar,
    NatsJetStream,
    RabbitMq,
    RocketMq,
    RedisStreams,
    Zenoh,
    Mqtt,
    Dds,
    FlinkStyleProcessor,
    TemporalStyleWorkflow,
    ArrowFlightBulk,
    VerifiedConsensusInterop,
}

impl ExtensionFacet {
    pub const fn stable_id(self) -> &'static str {
        match self {
            Self::KafkaCompatibleLog => "log.kafka-compatible",
            Self::Pulsar => "log.pulsar",
            Self::NatsJetStream => "stream.nats-jetstream",
            Self::RabbitMq => "queue.rabbitmq",
            Self::RocketMq => "queue.rocketmq",
            Self::RedisStreams => "stream.redis",
            Self::Zenoh => "edge.zenoh",
            Self::Mqtt => "edge.mqtt",
            Self::Dds => "edge.dds",
            Self::FlinkStyleProcessor => "compute.flink-style",
            Self::TemporalStyleWorkflow => "workflow.temporal-style",
            Self::ArrowFlightBulk => "bulk.arrow-flight",
            Self::VerifiedConsensusInterop => "verified.consensus-interop",
        }
    }
}

pub const FROZEN_EXTENSION_FACETS: &[ExtensionFacet] = &[
    ExtensionFacet::KafkaCompatibleLog,
    ExtensionFacet::Pulsar,
    ExtensionFacet::NatsJetStream,
    ExtensionFacet::RabbitMq,
    ExtensionFacet::RocketMq,
    ExtensionFacet::RedisStreams,
    ExtensionFacet::Zenoh,
    ExtensionFacet::Mqtt,
    ExtensionFacet::Dds,
    ExtensionFacet::FlinkStyleProcessor,
    ExtensionFacet::TemporalStyleWorkflow,
    ExtensionFacet::ArrowFlightBulk,
    ExtensionFacet::VerifiedConsensusInterop,
];

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn frozen_facets_have_unique_stable_ids() {
        let ids = FROZEN_EXTENSION_FACETS
            .iter()
            .map(|facet| facet.stable_id())
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), FROZEN_EXTENSION_FACETS.len());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleClass {
    Core,
    Business,
    Provider,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderId {
    SfExpress,
    WechatPay,
    Alipay,
    Miniapp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleActivation {
    Always,
    ProviderConfigured(ProviderId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleRequirement {
    SqlitePool,
    Module(&'static str),
    ProviderConfig(ProviderId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleFactoryId {
    Audit,
    Damage,
    Depreciation,
    Deposit,
    Device,
    ExcelImport,
    Invoice,
    Integration,
    Logistics,
    Model,
    Order,
    OrderLifecycleV2,
    OrderQueryV2,
    OrderReadCompatibility,
    Pricing,
    Quote,
    Procurement,
    Refund,
    Repair,
    Report,
    Roa,
    Settlement,
    Staff,
    Tax,
    UserSettings,
    WorkTask,
    Notify,
    Warehouse,
    WarehouseAdvanced,
    WarehouseRouting,
    Consent,
    Contract,
    Customer,
    Deletion,
    TwoFa,
    Barcode,
    Credit,
    Overdue,
    OpticalSop,
    Booking,
    Reservation,
    ReservationV2,
    R3Settlement,
    TenantGovernance,
    TenantPreview,
    TenantSimulation,
    SfExpress,
    WechatPay,
    Alipay,
    Miniapp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleDescriptor {
    pub registry_key: &'static str,
    pub metadata_name: &'static str,
    pub schema_name: &'static str,
    pub class: ModuleClass,
    pub activation: ModuleActivation,
    pub requirements: &'static [ModuleRequirement],
    pub factory: ModuleFactoryId,
}

const NONE: &[ModuleRequirement] = &[];
const SQLITE: &[ModuleRequirement] = &[ModuleRequirement::SqlitePool];
const DEVICE_MODULE: &[ModuleRequirement] = &[ModuleRequirement::Module("device")];
const PRICING: &[ModuleRequirement] = &[
    ModuleRequirement::Module("logistics"),
    ModuleRequirement::Module("warehouse_routing"),
];
const QUOTE: &[ModuleRequirement] = &[ModuleRequirement::Module("pricing")];
const WAREHOUSE_DEVICE_MODULES: &[ModuleRequirement] = &[
    ModuleRequirement::Module("warehouse"),
    ModuleRequirement::Module("device"),
];
const SF_EXPRESS: &[ModuleRequirement] =
    &[ModuleRequirement::ProviderConfig(ProviderId::SfExpress)];
const WECHAT_PAY: &[ModuleRequirement] =
    &[ModuleRequirement::ProviderConfig(ProviderId::WechatPay)];
const ALIPAY: &[ModuleRequirement] = &[ModuleRequirement::ProviderConfig(ProviderId::Alipay)];
const MINIAPP: &[ModuleRequirement] = &[ModuleRequirement::ProviderConfig(ProviderId::Miniapp)];

macro_rules! descriptor {
    ($registry_key:literal, $class:ident, $activation:expr, $requirements:expr, $factory:ident) => {
        ModuleDescriptor {
            registry_key: $registry_key,
            metadata_name: $registry_key,
            schema_name: $registry_key,
            class: ModuleClass::$class,
            activation: $activation,
            requirements: $requirements,
            factory: ModuleFactoryId::$factory,
        }
    };
    ($registry_key:literal => $runtime_identity:literal, $class:ident, $activation:expr, $requirements:expr, $factory:ident) => {
        ModuleDescriptor {
            registry_key: $registry_key,
            metadata_name: $runtime_identity,
            schema_name: $runtime_identity,
            class: ModuleClass::$class,
            activation: $activation,
            requirements: $requirements,
            factory: ModuleFactoryId::$factory,
        }
    };
}

pub const MODULE_DESCRIPTORS: &[ModuleDescriptor] = &[
    descriptor!("audit" => "feature-audit", Core, ModuleActivation::Always, NONE, Audit),
    descriptor!("damage", Business, ModuleActivation::Always, NONE, Damage),
    descriptor!(
        "depreciation",
        Business,
        ModuleActivation::Always,
        NONE,
        Depreciation
    ),
    descriptor!("deposit", Business, ModuleActivation::Always, NONE, Deposit),
    descriptor!("device" => "feature-device", Business, ModuleActivation::Always, NONE, Device),
    descriptor!("excel_import" => "feature-excel-import", Business, ModuleActivation::Always, DEVICE_MODULE, ExcelImport),
    descriptor!("invoice", Business, ModuleActivation::Always, NONE, Invoice),
    descriptor!(
        "integration",
        Core,
        ModuleActivation::Always,
        NONE,
        Integration
    ),
    descriptor!(
        "logistics",
        Business,
        ModuleActivation::Always,
        NONE,
        Logistics
    ),
    descriptor!("model", Business, ModuleActivation::Always, NONE, Model),
    descriptor!("order", Business, ModuleActivation::Always, NONE, Order),
    descriptor!(
        "order_lifecycle_v2",
        Business,
        ModuleActivation::Always,
        NONE,
        OrderLifecycleV2
    ),
    descriptor!(
        "order_query_v2",
        Business,
        ModuleActivation::Always,
        NONE,
        OrderQueryV2
    ),
    descriptor!(
        "order_read_compatibility",
        Business,
        ModuleActivation::Always,
        NONE,
        OrderReadCompatibility
    ),
    descriptor!(
        "pricing",
        Business,
        ModuleActivation::Always,
        PRICING,
        Pricing
    ),
    descriptor!("quote", Business, ModuleActivation::Always, QUOTE, Quote),
    descriptor!(
        "procurement",
        Business,
        ModuleActivation::Always,
        NONE,
        Procurement
    ),
    descriptor!("refund", Business, ModuleActivation::Always, NONE, Refund),
    descriptor!("repair", Business, ModuleActivation::Always, NONE, Repair),
    descriptor!("report", Business, ModuleActivation::Always, NONE, Report),
    descriptor!("roa", Business, ModuleActivation::Always, NONE, Roa),
    descriptor!(
        "settlement",
        Business,
        ModuleActivation::Always,
        NONE,
        Settlement
    ),
    descriptor!("staff", Business, ModuleActivation::Always, NONE, Staff),
    descriptor!("tax", Business, ModuleActivation::Always, NONE, Tax),
    descriptor!(
        "user_settings",
        Business,
        ModuleActivation::Always,
        NONE,
        UserSettings
    ),
    descriptor!(
        "work_task",
        Business,
        ModuleActivation::Always,
        NONE,
        WorkTask
    ),
    descriptor!("notify", Business, ModuleActivation::Always, NONE, Notify),
    descriptor!(
        "warehouse",
        Business,
        ModuleActivation::Always,
        NONE,
        Warehouse
    ),
    descriptor!(
        "warehouse_advanced",
        Business,
        ModuleActivation::Always,
        WAREHOUSE_DEVICE_MODULES,
        WarehouseAdvanced
    ),
    descriptor!(
        "warehouse_routing",
        Business,
        ModuleActivation::Always,
        NONE,
        WarehouseRouting
    ),
    descriptor!("consent", Business, ModuleActivation::Always, NONE, Consent),
    descriptor!(
        "contract",
        Business,
        ModuleActivation::Always,
        NONE,
        Contract
    ),
    descriptor!(
        "customer",
        Business,
        ModuleActivation::Always,
        NONE,
        Customer
    ),
    descriptor!(
        "deletion",
        Business,
        ModuleActivation::Always,
        NONE,
        Deletion
    ),
    descriptor!("two_fa", Business, ModuleActivation::Always, NONE, TwoFa),
    descriptor!("barcode", Business, ModuleActivation::Always, NONE, Barcode),
    descriptor!("credit", Business, ModuleActivation::Always, NONE, Credit),
    descriptor!("overdue", Business, ModuleActivation::Always, NONE, Overdue),
    descriptor!(
        "optical_sop",
        Business,
        ModuleActivation::Always,
        NONE,
        OpticalSop
    ),
    descriptor!("booking", Business, ModuleActivation::Always, NONE, Booking),
    descriptor!(
        "reservation",
        Business,
        ModuleActivation::Always,
        NONE,
        Reservation
    ),
    descriptor!(
        "reservation_v2",
        Business,
        ModuleActivation::Always,
        NONE,
        ReservationV2
    ),
    descriptor!(
        "r3_settlement",
        Business,
        ModuleActivation::Always,
        NONE,
        R3Settlement
    ),
    descriptor!("tenant_governance" => "feature-tenant-governance", Core, ModuleActivation::Always, NONE, TenantGovernance),
    descriptor!("tenant_preview" => "feature-tenant-preview", Core, ModuleActivation::Always, NONE, TenantPreview),
    descriptor!("tenant_simulation" => "feature-tenant-simulation", Core, ModuleActivation::Always, NONE, TenantSimulation),
    descriptor!(
        "sf_express",
        Provider,
        ModuleActivation::ProviderConfigured(ProviderId::SfExpress),
        SF_EXPRESS,
        SfExpress
    ),
    descriptor!(
        "wechat_pay",
        Provider,
        ModuleActivation::ProviderConfigured(ProviderId::WechatPay),
        WECHAT_PAY,
        WechatPay
    ),
    descriptor!(
        "alipay",
        Provider,
        ModuleActivation::ProviderConfigured(ProviderId::Alipay),
        ALIPAY,
        Alipay
    ),
    descriptor!(
        "miniapp",
        Provider,
        ModuleActivation::ProviderConfigured(ProviderId::Miniapp),
        MINIAPP,
        Miniapp
    ),
];

pub fn module_descriptors() -> &'static [ModuleDescriptor] {
    MODULE_DESCRIPTORS
}

#[cfg(test)]
mod tests {
    use super::{ModuleRequirement, module_descriptors};
    use std::collections::BTreeMap;

    #[test]
    fn migrated_registry_keys_do_not_claim_sqlite_storage() {
        let descriptors = module_descriptors();
        let descriptor = |key: &str| {
            descriptors
                .iter()
                .find(|descriptor| descriptor.registry_key == key)
                .expect("descriptor exists")
        };

        for key in [
            "audit",
            "consent",
            "deletion",
            "device",
            "report",
            "staff",
            "two_fa",
            "model",
            "warehouse",
            "procurement",
            "depreciation",
            "damage",
            "deposit",
            "refund",
            "repair",
            "invoice",
            "settlement",
            "tax",
            "credit",
            "overdue",
            "order",
            "notify",
            "contract",
            "barcode",
            "optical_sop",
            "booking",
            "reservation",
            "roa",
            "integration",
            "customer",
            "order_lifecycle_v2",
            "order_query_v2",
            "order_read_compatibility",
            "reservation_v2",
            "r3_settlement",
            "tenant_governance",
            "tenant_preview",
            "tenant_simulation",
        ] {
            assert_eq!(descriptor(key).requirements, &[]);
        }
        assert_eq!(
            descriptor("excel_import").requirements,
            &[ModuleRequirement::Module("device")]
        );
        assert_eq!(
            descriptor("warehouse_advanced").requirements,
            &[
                ModuleRequirement::Module("warehouse"),
                ModuleRequirement::Module("device"),
            ]
        );
        assert_eq!(
            descriptor("pricing").requirements,
            &[
                ModuleRequirement::Module("logistics"),
                ModuleRequirement::Module("warehouse_routing"),
            ]
        );
        assert_eq!(
            descriptor("quote").requirements,
            &[ModuleRequirement::Module("pricing")]
        );
        for key in [
            "device",
            "model",
            "warehouse",
            "excel_import",
            "warehouse_advanced",
            "procurement",
            "damage",
            "deposit",
            "refund",
            "repair",
            "invoice",
            "settlement",
            "tax",
            "credit",
            "overdue",
            "order",
            "notify",
            "contract",
            "barcode",
            "optical_sop",
            "booking",
            "reservation",
            "roa",
            "integration",
            "pricing",
            "quote",
            "customer",
            "order_lifecycle_v2",
            "order_query_v2",
            "order_read_compatibility",
            "reservation_v2",
            "r3_settlement",
            "tenant_governance",
            "tenant_preview",
            "tenant_simulation",
        ] {
            assert!(
                !descriptor(key)
                    .requirements
                    .contains(&ModuleRequirement::SqlitePool)
            );
        }
    }

    #[test]
    fn catalog_encodes_all_49_identity_planes_and_six_valid_unequal_mappings() {
        let descriptors = module_descriptors();
        assert_eq!(descriptors.len(), 50);
        assert!(
            descriptors
                .iter()
                .all(|descriptor| descriptor.metadata_name == descriptor.schema_name)
        );
        for key in [
            "order_lifecycle_v2",
            "order_read_compatibility",
            "customer",
            "integration",
            "quote",
        ] {
            assert!(
                descriptors
                    .iter()
                    .any(|descriptor| descriptor.registry_key == key)
            );
        }
        let unequal: BTreeMap<_, _> = descriptors
            .iter()
            .filter(|descriptor| descriptor.registry_key != descriptor.metadata_name)
            .map(|descriptor| (descriptor.registry_key, descriptor.metadata_name))
            .collect();
        assert_eq!(
            unequal,
            BTreeMap::from([
                ("audit", "feature-audit"),
                ("device", "feature-device"),
                ("excel_import", "feature-excel-import"),
                ("tenant_governance", "feature-tenant-governance"),
                ("tenant_preview", "feature-tenant-preview"),
                ("tenant_simulation", "feature-tenant-simulation"),
            ])
        );
    }
}

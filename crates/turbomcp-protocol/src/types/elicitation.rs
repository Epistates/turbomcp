//! User input elicitation types.
//!
//! These types are defined canonically in [`turbomcp_types`] (see
//! [`turbomcp_types::protocol`] for the request/result types and
//! [`turbomcp_types::protocol_schemas`] for the form schema types). This
//! module re-exports them and provides a handful of backward-compatible name
//! aliases for the pre-consolidation naming.
//!
//! ## Form Mode vs URL Mode
//!
//! | Aspect       | Form mode                              | URL mode                                    |
//! |--------------|----------------------------------------|---------------------------------------------|
//! | Data flow    | In-band (through MCP)                  | Out-of-band (external URL)                  |
//! | Use case     | Non-sensitive structured data          | Sensitive data, OAuth, credentials          |
//! | Security     | Data visible to MCP client             | Data **not** visible to MCP client          |

pub use turbomcp_types::{
    ElicitAction, ElicitRequestFormParams, ElicitRequestParams, ElicitRequestURLParams,
    ElicitResult, ElicitationCompleteNotification, ElicitationSchema, EnumOption, EnumSchema,
    MultiSelectItems, PrimitiveSchemaDefinition, TitledMultiSelectEnumSchema,
    TitledSingleSelectEnumSchema, URLElicitationRequiredError, UntitledMultiSelectEnumSchema,
    UntitledMultiSelectItems, UntitledSingleSelectEnumSchema,
};

/// Backward-compat alias — canonical name is [`ElicitAction`].
pub type ElicitationAction = ElicitAction;

/// Backward-compat alias — canonical name is [`ElicitRequestFormParams`].
pub type FormElicitRequestParams = ElicitRequestFormParams;

/// Backward-compat alias — canonical name is [`ElicitRequestURLParams`].
pub type URLElicitRequestParams = ElicitRequestURLParams;

/// Backward-compat alias — canonical name is [`ElicitationCompleteNotification`].
pub type ElicitationCompleteParams = ElicitationCompleteNotification;

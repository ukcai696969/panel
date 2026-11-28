use crate::models::EventEmittingModel;
use std::sync::LazyLock;

#[non_exhaustive]
pub enum UserEvent {
    /// Emitted after a user is created: admin user create
    /// (`backend/src/routes/api/admin/users`), registration
    /// (`backend/src/routes/api/auth/register.rs`), and OAuth first login
    /// (`backend/src/routes/api/auth/oauth/_oauth_provider_.rs`).
    Created { user: Box<super::User> },
}

#[async_trait::async_trait]
impl EventEmittingModel for super::User {
    type Event = UserEvent;

    fn get_event_emitter() -> &'static crate::events::EventEmitter<Self::Event> {
        static EVENT_EMITTER: LazyLock<crate::events::EventEmitter<UserEvent>> =
            LazyLock::new(crate::events::EventEmitter::default);

        &EVENT_EMITTER
    }
}

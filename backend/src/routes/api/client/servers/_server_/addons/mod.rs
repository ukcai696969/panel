use super::State;
use utoipa_axum::router::OpenApiRouter;

mod curseforge;
mod install;
mod installed;
mod modrinth;
mod search;
mod types;
mod versions;


pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .nest("/search", search::router(state))
        .nest("/versions", versions::router(state))
        .nest("/install", install::router(state))
        .nest("/installed", installed::router(state))
        .with_state(state.clone())
}

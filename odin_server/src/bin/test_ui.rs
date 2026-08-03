use odin_server::prelude::*;
use odin_actor::prelude::*;
use async_trait::async_trait;

pub struct TestUiService {}

#[async_trait]
impl SpaService for TestUiService {

    fn add_dependencies (&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder
            .add( build_service!( => UiService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()>  {
        spa.add_module( asset_uri!("test_ui.js"));

        Ok(())
    }
}

run_actor_system!( actor_system => {
    let _hserver = spawn_actor!( actor_system, "server",
        SpaServer::new(
            odin_server::load_config("spa_server.ron")?,
            "test_ui",
            SpaServiceList::new()
                .add( build_service!( => TestUiService{} ))
        )
    )?;
    Ok(())
});

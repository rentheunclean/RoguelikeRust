extern crate serde;
use rltk::{GameState, Rltk, Point};
use specs::prelude::*;
use specs::saveload::{SimpleMarker, SimpleMarkerAllocator};

mod components;
pub use components::*;
mod map;
pub use map::*;
mod player;
use player::*;
mod rect;
pub use rect::Rect;
mod visibility_system;
pub use visibility_system::VisibilitySystem;
mod ai_system;
pub use ai_system::MonsterAI;
mod map_indexing_system;
pub use map_indexing_system::MapIndexingSystem;
mod melee_combat_system;
pub use melee_combat_system::MeleeCombatSystem;
mod damage_system;
pub use damage_system::DamageSystem;
mod inventory_system;
pub use inventory_system::ItemCollectionSystem;
pub use inventory_system::ItemUseSystem;
pub use inventory_system::ItemRemoveSystem;
mod gui;
mod gamelog;
mod spawner;
mod saveload_system;
pub mod random_table;

#[derive(PartialEq, Copy, Clone)]
pub enum RunState { 
    AwaitingInput, 
    PreRun, 
    PlayerTurn, 
    MonsterTurn, 
    ShowInventory,
    ShowRemoveItem,
    ShowTargeting { range : i32, item : Entity },
    MainMenu { menu_selection : gui::MainMenuSelection },
    SaveGame,
    QuitGame,
    NextLevel,
    }

pub struct State 
{
    pub ecs: World,
}

impl State 
{
    fn run_systems(&mut self)
    {
        let mut vis = VisibilitySystem{};
        vis.run_now(&self.ecs);

        let mut mob = MonsterAI{};
        mob.run_now(&self.ecs);

        let mut mapindex = MapIndexingSystem{};
        mapindex.run_now(&self.ecs);

        let mut melee = MeleeCombatSystem{};
        melee.run_now(&self.ecs);

        let mut damage = DamageSystem{};
        damage.run_now(&self.ecs);

        let mut pickup = ItemCollectionSystem{};
        pickup.run_now(&self.ecs);

        let mut items = ItemUseSystem{};
        items.run_now(&self.ecs);

        let mut item_remove = ItemRemoveSystem{};
        item_remove.run_now(&self.ecs);

        self.ecs.maintain();
    }

    fn entities_to_remove_on_level_change(&mut self) -> Vec<Entity>
    {
        let entities = self.ecs.entities();
        let player = self.ecs.read_storage::<Player>();
        let backpack = self.ecs.read_storage::<InBackpack>();
        let equipped = self.ecs.read_storage::<Equipped>();
        let player_entity = self.ecs.fetch::<Entity>();

        let mut to_delete : Vec<Entity> = Vec::new();
        for entity in entities.join()
        {
            // retain player
            let p = player.get(entity);
            if let Some(_p) = p
            {
                continue;
            }

            // retain player equipment
            let bp = backpack.get(entity);
            if let Some(bp) = bp
            {
                if bp.owner == *player_entity
                {
                    continue
                }
            }

            let eq = equipped.get(entity);
            if let Some(eq) = eq
            {
                if eq.owner == *player_entity
                {
                    continue
                }
            }

            to_delete.push(entity);
        }

        to_delete
    }

    fn goto_next_level(&mut self)
    {
        let to_delete = self.entities_to_remove_on_level_change();
        for target in to_delete
        {
            self.ecs.delete_entity(target).expect("Unable to delete entity");
        }

        // build new map and place the player
        let worldmap;
        let current_depth;
        {
            let mut worldmap_resource = self.ecs.write_resource::<Map>();
            current_depth = worldmap_resource.depth;
            *worldmap_resource = Map::new_map_rooms_and_corridors(current_depth + 1);
            worldmap = worldmap_resource.clone();
        }

        // spawn enemies
        for room in worldmap.rooms.iter().skip(1)
        {
            spawner::spawn_room(&mut self.ecs, room, current_depth+1);
        }

        // place player and update resources
        let (player_x, player_y) = worldmap.rooms[0].center();
        let mut player_position = self.ecs.write_resource::<Point>();
        *player_position = Point::new(player_x, player_y);
        let mut position_components = self.ecs.write_storage::<Position>();
        let player_entity = self.ecs.fetch::<Entity>();
        let player_pos_comp = position_components.get_mut(*player_entity);
        if let Some(player_pos_comp) = player_pos_comp
        {
            player_pos_comp.x = player_x;
            player_pos_comp.y = player_y;
        }

        // mark the player's visibility as dirty
        let mut viewshed_components = self.ecs.write_storage::<Viewshed>();
        let vs = viewshed_components.get_mut(*player_entity);
        if let Some(vs) = vs
        {
            vs.dirty = true;
        }

        // Notify the player and give them health
        let mut gamelog = self.ecs.fetch_mut::<gamelog::GameLog>();
        gamelog.entries.push("You descend to the next level, and take a moment to heal.".to_string());
        let mut player_health_store = self.ecs.write_storage::<CombatStats>();
        let player_health = player_health_store.get_mut(*player_entity);
        if let Some(player_health) = player_health
        {
            player_health.hp = i32::max(player_health.hp, player_health.max_hp / 2);
        }
    }
}

impl GameState for State 
{
    fn tick(&mut self, ctx : &mut Rltk) 
    {
        let mut newrunstate;
        {
            let runstate = self.ecs.fetch::<RunState>();
            newrunstate = *runstate;
        }

        ctx.cls();
        
        match newrunstate 
        {
            RunState::MainMenu{..} => {}
            _ =>
            {
                draw_map(&self.ecs, ctx);

                {
                    let positions = self.ecs.read_storage::<Position>();
                    let renderables = self.ecs.read_storage::<Renderable>();
                    let map = self.ecs.fetch::<Map>();

                    let mut data = (&positions, &renderables).join().collect::<Vec<_>>();
                    data.sort_by(|&a, &b| b.1.render_order.cmp(&a.1.render_order) );
                    for (pos, render) in data.iter()
                    {
                        let idx = map.xy_idx(pos.x, pos.y);
                        if map.visible_tiles[idx]
                        {
                            ctx.set(pos.x, pos.y, render.fg, render.bg, render.glyph);
                        }
                    }

                    gui::draw_ui(&self.ecs, ctx);
                }
            }
        }

        match newrunstate
        {
            RunState::PreRun =>
            {
                self.run_systems();
                self.ecs.maintain();
                newrunstate = RunState::AwaitingInput;
            }
            RunState::AwaitingInput =>
            {
                self.run_systems();
                self.ecs.maintain();
                // TODO: shouldn't this return true/false and let THIS function handle what comes next?
                newrunstate = player_input(self, ctx);
            }
            RunState::PlayerTurn =>
            {
                self.run_systems();
                self.ecs.maintain();
                newrunstate = RunState::MonsterTurn;
            }
            RunState::MonsterTurn =>
            {
                self.run_systems();
                self.ecs.maintain();
                newrunstate = RunState::AwaitingInput;
            }
            RunState::ShowInventory =>
            {
                let result = gui::show_inventory(self, ctx);
                match result.0
                {
                    gui::ItemMenuResult::Cancel => newrunstate = RunState::AwaitingInput,
                    gui::ItemMenuResult::NoResponse => {}
                    gui::ItemMenuResult::Selected =>
                    {
                        let item_entity = result.1.unwrap();
                        let is_ranged = self.ecs.read_storage::<Ranged>();
                        let is_item_ranged = is_ranged.get(item_entity);
                        if let Some(is_item_ranged) = is_item_ranged
                        {
                            newrunstate = RunState::ShowTargeting{ 
                                    range: is_item_ranged.range,
                                    item: item_entity };
                        }
                        else {
                            let mut intent = self.ecs.write_storage::<WantsToUseItem>();
                            intent.insert(*self.ecs.fetch::<Entity>(), WantsToUseItem { item: item_entity, target: None })
                                .expect("Unable to insert intent");
                            newrunstate = RunState::PlayerTurn;
                        }
                    }
                }
            }
            RunState::ShowRemoveItem =>
            {
                let result = gui::remove_item_menu(self, ctx);
                match result.0
                {
                    gui::ItemMenuResult::Cancel => newrunstate = RunState::AwaitingInput,
                    gui::ItemMenuResult::NoResponse => {}
                    gui::ItemMenuResult::Selected => 
                    {
                        let item_entity = result.1.unwrap();
                        let mut intent = self.ecs.write_storage::<WantsToRemoveItem>();
                        intent.insert(*self.ecs.fetch::<Entity>(), WantsToRemoveItem{ item: item_entity })
                            .expect("Unable to insert intent");
                        newrunstate = RunState::PlayerTurn;
                    }
                }
            }
            RunState::ShowTargeting{range, item} =>
            {
                let result = gui::ranged_target(self, ctx, range);
                match result.0
                {
                    gui::ItemMenuResult::Cancel => newrunstate = RunState::AwaitingInput,
                    gui::ItemMenuResult::NoResponse => {}
                    gui::ItemMenuResult::Selected =>
                    {
                        let mut intent = self.ecs.write_storage::<WantsToUseItem>();
                        intent.insert(*self.ecs.fetch::<Entity>(), WantsToUseItem{ item, target: result.1 })
                                        .expect("Unable to insert intent");
                        newrunstate = RunState::PlayerTurn;
                    }
                }
            }
            RunState::MainMenu{..} =>
            {
                let result = gui::main_menu(self, ctx);
                match result 
                {
                    gui::MainMenuResult::NoSelection{ selected } => 
                        newrunstate = RunState::MainMenu{ menu_selection: selected },
                    gui::MainMenuResult::Selected{ selected } =>
                    {
                        match selected
                        {
                            gui::MainMenuSelection::NewGame => newrunstate = RunState::PreRun,
                            gui::MainMenuSelection::LoadGame => 
                            {
                                saveload_system::load_game(&mut self.ecs);
                                newrunstate = RunState::AwaitingInput;
                                //saveload_system::delete_save();
                            }
                            gui::MainMenuSelection::Quit => { ::std::process::exit(0); }
                        }
                    }
                }
            }
            RunState::SaveGame =>
            {
                saveload_system::save_game(&mut self.ecs);
                newrunstate = RunState::AwaitingInput;
            }
            RunState::QuitGame =>
            {
                newrunstate = RunState::MainMenu{ menu_selection : gui::MainMenuSelection::NewGame };
            }
            RunState::NextLevel =>
            {
                self.goto_next_level();
                newrunstate = RunState::PreRun;
            }
        }

        {
            let mut runwriter = self.ecs.write_resource::<RunState>();
            *runwriter = newrunstate;
        }

        // TODO: can this be called in the damage system itself?
        damage_system::delete_the_dead(&mut self.ecs);
    }
}

fn main() -> rltk::BError 
{
    use rltk::RltkBuilder;
    let context = RltkBuilder::simple80x50()
        .with_title("Roguelike Rust")
        .build()?;

    let mut gs = State 
    { 
        ecs: World::new(),
    };
    gs.ecs.register::<Position>();
    gs.ecs.register::<Renderable>();
    gs.ecs.register::<Player>();
    gs.ecs.register::<Viewshed>();
    gs.ecs.register::<Monster>();
    gs.ecs.register::<Item>();
    gs.ecs.register::<InBackpack>();
    gs.ecs.register::<WantsToPickupItem>();
    gs.ecs.register::<WantsToUseItem>();
    gs.ecs.register::<WantsToRemoveItem>();
    gs.ecs.register::<ProvidesHealing>();
    gs.ecs.register::<InflictsDamage>();
    gs.ecs.register::<Ranged>();
    gs.ecs.register::<Consumable>();
    gs.ecs.register::<Name>();
    gs.ecs.register::<BlocksTile>();
    gs.ecs.register::<CombatStats>();
    gs.ecs.register::<WantsToMelee>();
    gs.ecs.register::<SufferDamage>();
    gs.ecs.register::<Confusion>();
    gs.ecs.register::<AreaOfEffect>();
    gs.ecs.register::<SimpleMarker<SerializeMe>>();
    gs.ecs.register::<SerializationHelper>();
    gs.ecs.register::<Equippable>();
    gs.ecs.register::<Equipped>();
    gs.ecs.register::<MeleePowerBonus>();
    gs.ecs.register::<DefenseBonus>();

    gs.ecs.insert(SimpleMarkerAllocator::<SerializeMe>::new());

    let map : Map = Map::new_map_rooms_and_corridors(1);
    let(player_x, player_y) = map.rooms[0].center();

    let player_entity = spawner::player(&mut gs.ecs, player_x, player_y);
    
    gs.ecs.insert(rltk::RandomNumberGenerator::new());
    for room in map.rooms.iter().skip(1)
    {
        spawner::spawn_room(&mut gs.ecs, room, 1);
    }
        
    gs.ecs.insert( RunState::MainMenu{ menu_selection : gui::MainMenuSelection::NewGame } );
    gs.ecs.insert(map);
    gs.ecs.insert(Point::new(player_x, player_y));
    gs.ecs.insert(player_entity);

    gs.ecs.insert(gamelog::GameLog{entries : vec!["Welcome to Roguelike Rust".to_string()]});

    rltk::main_loop(context, gs)
}

use specs::prelude::*;
use super::{ WantsToPickupItem, Name, InBackpack, Position, gamelog::GameLog, Map,
            WantsToUseItem, WantsToRemoveItem, ProvidesHealing, InflictsDamage, 
            SufferDamage, CombatStats, 
            Confusion, Consumable, AreaOfEffect, Equippable, Equipped };

pub struct ItemCollectionSystem {}

impl<'a> System<'a> for ItemCollectionSystem
{
    #[allow(clippy::type_complexity)]
    type SystemData = ( ReadExpect<'a, Entity>,
                        WriteExpect<'a, GameLog>,
                        WriteStorage<'a, WantsToPickupItem>,
                        WriteStorage<'a, Position>,
                        ReadStorage<'a, Name>,
                        WriteStorage<'a, InBackpack>
                        );

    fn run(&mut self, data : Self::SystemData)
    {
        let (player_entity, mut gamelog, mut wants_pickup, mut positions, names, mut backpack) = data;

        for pickup in wants_pickup.join()
        {
            positions.remove(pickup.item);
            backpack.insert(pickup.item, InBackpack{ owner: pickup.collected_by })
                .expect("Unable to insert backpack entry");
            
            if pickup.collected_by == *player_entity 
            {
                gamelog.entries.push(format!("You pick up the {}.", names.get(pickup.item).unwrap().name));
            }
        }

        wants_pickup.clear();
    }
}

pub struct ItemUseSystem {}

impl<'a> System<'a> for ItemUseSystem
{
    #[allow(clippy::type_complexity)]
    type SystemData = ( ReadExpect<'a, Entity>,
                        WriteExpect<'a, GameLog>,
                        ReadExpect<'a, Map>,
                        Entities<'a>,
                        WriteStorage<'a, WantsToUseItem>,
                        ReadStorage<'a, Name>,
                        ReadStorage<'a, Consumable>,
                        ReadStorage<'a, ProvidesHealing>,
                        ReadStorage<'a, InflictsDamage>,
                        WriteStorage<'a, SufferDamage>,
                        WriteStorage<'a, Confusion>,
                        ReadStorage<'a, AreaOfEffect>,
                        WriteStorage<'a, CombatStats>,
                        ReadStorage<'a, Equippable>,
                        WriteStorage<'a, Equipped>,
                        WriteStorage<'a, InBackpack>
                        );

    fn run(&mut self, data : Self::SystemData)
    {
        let (player_entity, mut gamelog, map, entities, mut wants_use, 
            names, consumables, healing, inflict_damage, mut suffer_damage, 
            mut confused, aoe, mut combat_stats, equippable, mut equipped, mut backpack) = data;

        for (entity, useitem) in (&entities, &wants_use).join()
        {
            let mut used_item = false;

            // Targeting 
            let mut targets : Vec<Entity> = Vec::new();
            match useitem.target 
            {
                None => { targets.push( *player_entity ); }
                Some(target) => 
                {
                    let area_effect = aoe.get(useitem.item);
                    match area_effect
                    {
                        None => 
                        {
                            // Single target in Tile
                            let idx = map.xy_idx(target.x, target.y);
                            for mob in map.tile_content[idx].iter()
                            {
                                targets.push(*mob);
                            }
                        }
                        Some(area_effect) => 
                        {
                            // AoE
                            let mut blast_tiles = rltk::field_of_view(target, area_effect.radius, &*map);
                            blast_tiles.retain(|p| p.x > 0 && p.x < map.width-1 && p.y > 0 && p.y < map.height-1);
                            for tile_idx in blast_tiles.iter()
                            {
                                let idx = map.xy_idx(tile_idx.x, tile_idx.y);
                                for mob in map.tile_content[idx].iter()
                                {
                                    targets.push(*mob);
                                }
                            }
                        }
                    }
                }
            }

            // equipment
            let item_equippable = equippable.get(useitem.item);
            match item_equippable
            {
                None => {}
                Some(can_equip) =>
                {
                    let target_slot = can_equip.slot;
                    let target = targets[0];

                    // remove any items the target has in the item's slot
                    let mut to_unequip : Vec<Entity> = Vec::new();
                    for (item_entity, already_equipped, name) in (&entities, &equipped, &names).join()
                    {
                        if already_equipped.owner == target && already_equipped.slot == target_slot
                        {
                            to_unequip.push(item_entity);
                            if target == *player_entity
                            {
                                gamelog.entries.push(format!("You unequip {}.", name.name));
                            }
                        }
                    }
                    for item in to_unequip.iter()
                    {
                        equipped.remove(*item);
                        backpack.insert(*item, InBackpack { owner : target })
                            .expect("Unable to insert backback entry");
                    }

                    // wield the item
                    equipped.insert(useitem.item, Equipped{ owner: target, slot: target_slot })
                        .expect("Unable to insert equipped component");
                    backpack.remove(useitem.item);
                    if target == *player_entity
                    {
                        gamelog.entries.push(format!("You equip {}.", names.get(useitem.item).unwrap().name));
                    }
                }
            }


            // apply healing
            let item_heals = healing.get(useitem.item);
            match item_heals
            {
                None => {}
                Some(healer) => 
                {
                    for target in targets.iter()
                    {
                        let stats = combat_stats.get_mut(*target);
                        if let Some(stats) = stats
                        {
                            stats.hp = i32::min(stats.max_hp, stats.hp + healer.heal_amount);
                            if entity == *player_entity
                            {
                                gamelog.entries.push(format!("You use the {}, healing {} hp",
                                    names.get(useitem.item).unwrap().name, healer.heal_amount));
                            }
                            used_item = true;
                        }
                    }
                }
            }

            // apply damage
            let item_damage = inflict_damage.get(useitem.item);
            match item_damage
            {
                None => {}
                Some(damage) =>
                {
                    for mob in targets.iter()
                    {
                        SufferDamage::new_damage(&mut suffer_damage, *mob, damage.damage);
                        if entity == *player_entity
                        {
                            let mob_name = names.get(*mob).unwrap();
                            let item_name = names.get(useitem.item).unwrap();
                            gamelog.entries.push(format!("You use {} on {}, inflicting {} hp.", 
                                item_name.name, mob_name.name, damage.damage));
                        }

                        used_item = true;
                    }
                }
            }

            // apply Confusion
            let mut add_confusion = Vec::new();
            {
                let causes_confusion = confused.get(useitem.item);
                match causes_confusion
                {
                    None => {}
                    Some(confusion) =>
                    {
                        for mob in targets.iter()
                        {
                            add_confusion.push((*mob, confusion.turns ));
                            if entity == *player_entity
                            {
                                let mob_name = names.get(*mob).unwrap();
                                let item_name = names.get(useitem.item).unwrap();
                                gamelog.entries.push(format!("You use {} on {}, confusing them.", 
                                    item_name.name, mob_name.name))
                            }

                            used_item = true;
                        }
                    }
                }
            }
            for mob in add_confusion.iter()
            {
                confused.insert(mob.0, Confusion{ turns: mob.1 }).expect("Unable to insert status");
            }
            
            // consume item if necesssary
            if used_item 
            {
                let consumable = consumables.get(useitem.item);
                match consumable 
                {
                    None => {}
                    Some(_) => 
                    {
                        entities.delete(useitem.item).expect("Delete failed");
                    }
                }
            }
        }

        wants_use.clear();
    }
}

pub struct ItemRemoveSystem {}

impl<'a> System<'a> for ItemRemoveSystem
{
    #[allow(clippy::type_complexity)]
    type SystemData = ( Entities<'a>,
                        WriteStorage<'a, WantsToRemoveItem>,
                        WriteStorage<'a, Equipped>,
                        WriteStorage<'a, InBackpack>,
                        );

    fn run(&mut self, data : Self::SystemData)
    {
        let (entities, mut wants_remove, mut equipped, mut backpack) = data;

        for (entity, to_remove) in (&entities, &wants_remove).join()
        {
            equipped.remove(to_remove.item);
            backpack.insert(to_remove.item, InBackpack{ owner: entity })
                .expect("Unable to insert backpack");
        }

        wants_remove.clear();
    }
}
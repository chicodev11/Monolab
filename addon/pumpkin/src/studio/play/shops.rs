//! Studio shops (`shop` files, rules in `studio_format::shop`, same as the Paper add-on's `StudioShops`): the `shop`
//! action opens a chest screen; left click buys, right click sells, shift + right click sells all. Money is a player
//! variable. Vault doesn't exist on Pumpkin, so a `vault` shop says so in its status and doesn't open.

use super::{Run, TAG, give, id_text, item_key};
use crate::studio::Studio;
use pumpkin_plugin_api::common::ClickType;
use pumpkin_plugin_api::gui::Gui;
use pumpkin_plugin_api::persistent_data::PersistentDataHolder;
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::{Inventory, ItemStack, Player, Screen, Server};
use serde_json::{Value, json};
use studio_format::shop::{self, Good};

impl Studio {
    /// The `shop` action.
    pub(super) async fn open_shop(&mut self, server: &Server, player: &Player, id: &str) {
        let Some(data) = self.data_of("shop", id) else {
            player.send_system_message(TextComponent::text("That shop doesn't exist (any more)."), false);
            return;
        };
        if !shop::problems(&data).is_empty() {
            player.send_system_message(TextComponent::text("That shop has a problem; see the studio."), false);
            return;
        }
        if shop::currency(&data) == "vault" {
            player.send_system_message(TextComponent::text("That shop uses Vault money, which only Paper servers have."), false);
            return;
        }
        let rows = shop::rows(&data);
        let screen = match rows {
            1 => Screen::Generic9x1,
            2 => Screen::Generic9x2,
            4 => Screen::Generic9x4,
            5 => Screen::Generic9x5,
            6 => Screen::Generic9x6,
            _ => Screen::Generic9x3,
        };
        let run = Run::of(player, vec![("shop".into(), id.to_string())]);
        let title = self.text_for(server, &studio_format::fields::text(&data, "title", "Shop"), &run);
        let gui = Gui::new(screen, title);
        gui.set_allow_grab_items(false).await;
        gui.set_allow_put_items(false).await;
        // The screen goes to the player; its inventory stays here to draw on after each click.
        let inventory = gui.get_inventory().await;
        self.draw_shop(server, player, &data, &inventory).await;
        player.open_gui(gui).await;
        let key = id_text(player.get_id());
        self.play.menus.remove(&key);
        self.play.shops.insert(key, (id.to_string(), inventory));
    }

    async fn draw_shop(&mut self, server: &Server, player: &Player, data: &studio_format::fields::Data, inventory: &Inventory) {
        let balance = self.balance(player, data);
        let name = shop::money_name(data);
        let run = Run::of(player, Vec::new());
        let size = shop::rows(data) * 9;
        for (at, good) in shop::goods(data).iter().enumerate().take(size as usize) {
            let stack = match self.stack(server, &good.item, good.amount, &run).await {
                Some(stack) => stack,
                None => ItemStack::new(&item_key("barrier").unwrap_or_else(|| "minecraft:barrier".into()), 1),
            };
            let mut lore = stack.get_lore().await;
            lore.extend(shop::lore(good, &name, balance).iter().map(|line| self.text_for(server, line, &run)));
            stack.set_lore(lore).await;
            inventory.set_item(at as u32, Some(stack)).await;
        }
    }

    /// A click in an open shop. Returns false when the player has none open.
    pub(super) async fn shop_click(&mut self, server: &Server, player: &Player, slot: i32, click: ClickType) -> bool {
        let key = id_text(player.get_id());
        let Some((id, _)) = self.play.shops.get(&key) else { return false };
        let id = id.clone();
        let Some(data) = self.data_of("shop", &id) else { return true };
        let goods = shop::goods(&data);
        let Some(good) = usize::try_from(slot).ok().and_then(|at| goods.get(at)).cloned() else { return true };
        let message = match click {
            ClickType::Left | ClickType::ShiftLeft => self.buy(server, player, &data, &good).await,
            ClickType::Right => self.sell(player, &data, &good, false).await,
            ClickType::ShiftRight => self.sell(player, &data, &good, true).await,
            _ => return true,
        };
        let run = Run::of(player, Vec::new());
        let text = self.text_for(server, &message, &run);
        player.show_actionbar(text);
        if let Some((_, inventory)) = self.play.shops.remove(&key) {
            self.draw_shop(server, player, &data, &inventory).await;
            self.play.shops.insert(key, (id, inventory));
        }
        true
    }

    pub(super) fn shop_closed(&mut self, player: &Player) {
        self.play.shops.remove(&id_text(player.get_id()));
    }

    fn balance(&mut self, player: &Player, data: &studio_format::fields::Data) -> f64 {
        self.load_vars();
        let name = shop::variable(data);
        match self.play.vars_players.get(&id_text(player.get_id())).and_then(|vars| vars.get(&name)) {
            Some(Value::Number(one)) => one.as_f64().unwrap_or(0.0),
            Some(Value::String(text)) => text.trim().parse().unwrap_or(0.0),
            _ => 0.0,
        }
    }

    fn pay(&mut self, player: &Player, data: &studio_format::fields::Data, amount: f64) {
        let now = self.balance(player, data);
        self.play.vars_players.entry(id_text(player.get_id())).or_default().insert(shop::variable(data), json!(now + amount));
        self.play.vars_dirty = true;
    }

    async fn buy(&mut self, server: &Server, player: &Player, data: &studio_format::fields::Data, good: &Good) -> String {
        let name = shop::money_name(data);
        if good.buy <= 0.0 {
            return "<red>This shop doesn't sell that.".into();
        }
        if self.balance(player, data) < good.buy {
            return format!("<red>Not enough {name}: it costs {}.", shop::money_text(good.buy));
        }
        let run = Run::of(player, Vec::new());
        let Some(stack) = self.stack(server, &good.item, good.amount, &run).await else {
            return "<red>That item doesn't exist (any more).".into();
        };
        self.pay(player, data, -good.buy);
        give(player, stack).await;
        format!("<green>Bought {} {} for {} {name}.", good.amount, good.item, shop::money_text(good.buy))
    }

    async fn sell(&mut self, player: &Player, data: &studio_format::fields::Data, good: &Good, all: bool) -> String {
        let name = shop::money_name(data);
        if good.sell <= 0.0 {
            return "<red>This shop doesn't buy that.".into();
        }
        let mut have = 0u32;
        for slot in 0..36u8 {
            if let Some(stack) = player.get_inventory_item(slot)
                && same(&stack, &good.item).await
            {
                have += u32::from(stack.get_count().await);
            }
        }
        let times = if all { have / good.amount } else { u32::from(have >= good.amount) };
        if times == 0 {
            return format!("<red>You need {} {} to sell.", good.amount, good.item);
        }
        let mut left = times * good.amount;
        for slot in 0..36u8 {
            if left == 0 {
                break;
            }
            let Some(stack) = player.get_inventory_item(slot) else { continue };
            if !same(&stack, &good.item).await {
                continue;
            }
            let count = u32::from(stack.get_count().await);
            let take = left.min(count);
            left -= take;
            if take == count {
                player.set_inventory_item(slot, None).await;
            } else {
                stack.set_count((count - take) as u8).await;
                player.set_inventory_item(slot, Some(stack)).await;
            }
        }
        let earned = f64::from(times) * good.sell;
        self.pay(player, data, earned);
        format!("<green>Sold {} {} for {} {name}.", times * good.amount, good.item, shop::money_text(earned))
    }
}

/// Whether a stack is the good: our items by id, vanilla ones by registry key (and not one of ours).
async fn same(stack: &ItemStack, item: &str) -> bool {
    let ours = stack.get_string(TAG, "item").await;
    if let Some(reference) = item.strip_prefix("studio:") {
        let id = reference.strip_prefix("item/").unwrap_or(reference);
        return ours.as_deref() == Some(id);
    }
    match item_key(item) {
        Some(key) => ours.is_none() && stack.get_registry_key().await == key,
        None => false,
    }
}

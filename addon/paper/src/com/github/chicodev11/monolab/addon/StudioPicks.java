package com.github.chicodev11.monolab.addon;

import io.papermc.paper.event.player.PlayerPickBlockEvent;
import io.papermc.paper.event.player.PlayerPickEntityEvent;
import java.util.function.Predicate;
import org.bukkit.GameMode;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.PlayerInventory;

/**
 * Middle click (pick block) on studio blocks and furniture: their own item instead of the note block, tripwire,
 * barrier or nothing the game would pick. Like vanilla: one in the hotbar is selected, one further in the inventory
 * comes to the hotbar, and in creative a new one is given when there is none.
 *
 * <p>Kept apart from {@link StudioBlocks} because the pick events are newer than the oldest servers the add-on
 * runs on: registering this can fail alone.
 */
final class StudioPicks implements Listener {
    /** The item to pick, and how to know one already in the inventory (a stack may differ, like a renamed one). */
    record Pick(ItemStack item, Predicate<ItemStack> same) {}

    private final StudioBlocks blocks;
    private final StudioFurniture furniture;

    StudioPicks(StudioBlocks blocks, StudioFurniture furniture) {
        this.blocks = blocks;
        this.furniture = furniture;
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onPickBlock(PlayerPickBlockEvent event) {
        Pick pick = blocks.pick(event.getBlock());
        if (pick == null) {
            pick = furniture.pick(event.getBlock());
        }
        if (pick != null) {
            event.setCancelled(true);
            give(event.getPlayer(), pick);
        }
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onPickEntity(PlayerPickEntityEvent event) {
        Pick pick = furniture.pick(event.getEntity());
        if (pick != null) {
            event.setCancelled(true);
            give(event.getPlayer(), pick);
        }
    }

    /** Vanilla's pick: select it in the hotbar, bring it there from the inventory, or (creative) put a new one there. */
    static void give(Player player, Pick pick) {
        PlayerInventory inventory = player.getInventory();
        int found = -1;
        for (int slot = 0; slot < 36 && found < 0; slot++) {
            ItemStack stack = inventory.getItem(slot);
            if (stack != null && !stack.getType().isAir() && pick.same().test(stack)) {
                found = slot;
            }
        }
        if (found >= 0 && found < 9) {
            inventory.setHeldItemSlot(found);
            return;
        }
        if (found < 0 && player.getGameMode() != GameMode.CREATIVE) {
            return;
        }
        int target = hotbarSlot(inventory);
        ItemStack there = inventory.getItem(target);
        if (found >= 0) {
            inventory.setItem(target, inventory.getItem(found));
            inventory.setItem(found, there);
        } else {
            if (there != null && !there.getType().isAir()) {
                // What was there moves to a free slot, as vanilla does; with none free it is replaced (creative).
                int free = inventory.firstEmpty();
                if (free >= 0 && free < 36) {
                    inventory.setItem(free, there);
                }
            }
            inventory.setItem(target, pick.item());
        }
        inventory.setHeldItemSlot(target);
    }

    /** Vanilla's suitable hotbar slot: the held one when empty, else the first empty one, else the held one. */
    private static int hotbarSlot(PlayerInventory inventory) {
        int held = inventory.getHeldItemSlot();
        if (empty(inventory.getItem(held))) {
            return held;
        }
        for (int slot = 0; slot < 9; slot++) {
            if (empty(inventory.getItem(slot))) {
                return slot;
            }
        }
        return held;
    }

    private static boolean empty(ItemStack stack) {
        return stack == null || stack.getType().isAir();
    }
}

package com.github.chicodev11.monolab.addon;

import io.papermc.paper.event.player.PlayerPurchaseEvent;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import org.bukkit.Bukkit;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.Merchant;
import org.bukkit.inventory.MerchantRecipe;

/**
 * Studio trade lists ({@code trade}): the {@code trade} action opens a villager trade screen of the file's offers.
 * {@code uses} counts per player (kept while the server runs) until {@code restockMinutes} pass. Same rules as
 * {@code crates/studio-format/src/trade.rs}.
 */
final class StudioTrades implements Listener {
    private static final int MOST_OFFERS = 64;
    private static volatile StudioTrades current;

    private final StudioActions actions;
    private final Studio studio;
    private volatile Map<String, Map<String, Object>> trades = Map.of();
    /** Open screens: which file and which offers (index in the file) they show. */
    private final Map<Merchant, Open> open = java.util.Collections.synchronizedMap(new IdentityHashMap<>());
    /** player|trade|offer → times taken and when the first of them was. */
    private final Map<String, long[]> taken = new ConcurrentHashMap<>();

    private record Open(String trade, List<Integer> offers) {}

    StudioTrades(StudioActions actions, Studio studio) {
        this.actions = actions;
        this.studio = studio;
    }

    static StudioTrades get() {
        return current;
    }

    void register() {
        current = this;
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "trade";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                Map<String, Map<String, Object>> next = new HashMap<>();
                for (Studio.Resource one : resources) {
                    String problem = problem(one.data());
                    if (problem != null) {
                        studio.problem("trade", one.id(), problem);
                    }
                    next.put(one.id(), one.data());
                }
                trades = Map.copyOf(next);
            }
        });
    }

    void stop() {
        current = null;
    }

    private static List<Map<String, Object>> offers(Map<String, Object> data) {
        List<Map<String, Object>> out = new ArrayList<>();
        if (data.get("offers") instanceof List<?> list) {
            for (Object raw : list) {
                if (out.size() >= MOST_OFFERS) {
                    break;
                }
                if (raw instanceof Map<?, ?> one) {
                    out.add(StudioActions.map(one));
                }
            }
        }
        return out;
    }

    static String problem(Map<String, Object> data) {
        List<Map<String, Object>> list = offers(data);
        if (list.isEmpty()) {
            return "A trade list needs \"offers\": what players give and get.";
        }
        for (int at = 0; at < list.size(); at++) {
            if (StudioText.text(list.get(at), "buy", "").isBlank() || StudioText.text(list.get(at), "sell", "").isBlank()) {
                return "Offer " + (at + 1) + " needs an item to give (\"buy\") and one to get (\"sell\").";
            }
        }
        return null;
    }

    /** The {@code trade} action. On the player's thread. The older merchant calls still work on every 1.21 server. */
    @SuppressWarnings("deprecation")
    void open(Player player, String id) {
        Map<String, Object> data = trades.get(id);
        if (data == null) {
            return;
        }
        Merchant merchant = Bukkit.createMerchant(StudioText.render(StudioText.text(data, "title", "Trades"), player));
        List<MerchantRecipe> recipes = new ArrayList<>();
        List<Integer> shown = new ArrayList<>();
        List<Map<String, Object>> list = offers(data);
        long restock = StudioText.number(data, "restockMinutes", 0, 0, 60 * 24 * 30) * 60_000L;
        for (int at = 0; at < list.size(); at++) {
            Map<String, Object> one = list.get(at);
            ItemStack sell = actions.stack(StudioText.text(one, "sell", "").trim(), StudioText.number(one, "sellAmount", 1, 1, 64));
            ItemStack buy = actions.stack(StudioText.text(one, "buy", "").trim(), StudioText.number(one, "buyAmount", 1, 1, 64));
            if (sell == null || buy == null) {
                continue;
            }
            int limit = StudioText.number(one, "uses", 0, 0, 100_000);
            int used = used(player, id, at, restock);
            MerchantRecipe recipe = new MerchantRecipe(sell, Math.min(used, limit == 0 ? 0 : limit), limit == 0 ? Integer.MAX_VALUE : limit, false, 0, 0f, true);
            recipe.addIngredient(buy);
            String second = StudioText.text(one, "buy2", "").trim();
            if (!second.isEmpty()) {
                ItemStack buy2 = actions.stack(second, StudioText.number(one, "buy2Amount", 1, 1, 64));
                if (buy2 != null) {
                    recipe.addIngredient(buy2);
                }
            }
            recipes.add(recipe);
            shown.add(at);
        }
        if (recipes.isEmpty()) {
            return;
        }
        merchant.setRecipes(recipes);
        open.put(merchant, new Open(id, shown));
        player.openMerchant(merchant, true);
    }

    private int used(Player player, String trade, int offer, long restock) {
        long[] one = taken.get(player.getUniqueId() + "|" + trade + "|" + offer);
        if (one == null) {
            return 0;
        }
        if (restock > 0 && System.currentTimeMillis() - one[1] >= restock) {
            taken.remove(player.getUniqueId() + "|" + trade + "|" + offer);
            return 0;
        }
        return (int) one[0];
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onPurchase(PlayerPurchaseEvent event) {
        Open screen = open.get(event.getMerchant());
        if (screen == null) {
            return;
        }
        int index = event.getMerchant().getRecipes().indexOf(event.getTrade());
        if (index < 0 || index >= screen.offers().size()) {
            return;
        }
        String key = event.getPlayer().getUniqueId() + "|" + screen.trade() + "|" + screen.offers().get(index);
        taken.compute(key, (name, was) -> was == null ? new long[] {1, System.currentTimeMillis()} : new long[] {was[0] + 1, was[1]});
    }

    @EventHandler(priority = EventPriority.MONITOR)
    public void onClose(org.bukkit.event.inventory.InventoryCloseEvent event) {
        if (event.getInventory() instanceof org.bukkit.inventory.MerchantInventory inventory) {
            open.remove(inventory.getMerchant());
        }
    }
}

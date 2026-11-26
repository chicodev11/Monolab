package com.github.chicodev11.monolab.addon;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.format.TextDecoration;
import org.bukkit.Bukkit;
import org.bukkit.Material;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.inventory.ClickType;
import org.bukkit.event.inventory.InventoryClickEvent;
import org.bukkit.event.inventory.InventoryDragEvent;
import org.bukkit.inventory.Inventory;
import org.bukkit.inventory.InventoryHolder;
import org.bukkit.inventory.ItemStack;
import org.bukkit.plugin.Plugin;

/**
 * Studio shops ({@code shop}): the {@code shop} action opens a chest screen of goods. Left click buys, right click
 * sells, shift + right click sells all. Money is a player variable or the Vault economy. Same rules as
 * {@code crates/studio-format/src/shop.rs}. Also the {@code money} action and condition (Vault).
 */
final class StudioShops implements Listener {
    private static final int MOST_GOODS = 54;
    private static final List<String> CURRENCIES = List.of("variable", "vault");

    private final Plugin plugin;
    private final StudioActions actions;
    private final Studio studio;
    private volatile Map<String, Map<String, Object>> shops = Map.of();

    record Good(String item, int amount, double buy, double sell) {}

    /** Marks our shop screens. */
    static final class ShopHolder implements InventoryHolder {
        final String shop;
        Inventory inventory;

        ShopHolder(String shop) {
            this.shop = shop;
        }

        @Override
        public Inventory getInventory() {
            return inventory;
        }
    }

    StudioShops(Plugin plugin, StudioActions actions, Studio studio) {
        this.plugin = plugin;
        this.actions = actions;
        this.studio = studio;
    }

    void register() {
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "shop";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                Map<String, Map<String, Object>> next = new HashMap<>();
                for (Studio.Resource one : resources) {
                    String problem = problem(one.data());
                    if (problem != null) {
                        studio.problem("shop", one.id(), problem);
                    } else if ("vault".equals(currency(one.data())) && !StudioEconomy.ready()) {
                        studio.problem("shop", one.id(), "It uses Vault money, but no Vault economy plugin is running.");
                    }
                    next.put(one.id(), one.data());
                }
                shops = Map.copyOf(next);
            }
        });
        actions.action("shop", (one, run) -> {
            Player player = run.player();
            if (player != null) {
                player.getScheduler().run(plugin, task -> open(player, StudioText.text(one, "shop", "").trim()), null);
            }
        });
        actions.action("money", (one, run) -> {
            Player player = run.player();
            if (player == null) {
                return;
            }
            double amount = amount(one);
            String op = StudioText.text(one, "op", "give");
            if ("set".equals(op)) {
                Double now = StudioEconomy.balance(player);
                if (now != null) {
                    StudioEconomy.change(player, amount - now);
                }
            } else {
                StudioEconomy.change(player, "take".equals(op) ? -amount : amount);
            }
        });
        actions.condition("money", (one, run) -> {
            Player player = run.player();
            Double balance = player == null ? null : StudioEconomy.balance(player);
            return balance != null && balance >= amount(one);
        });
    }

    private static double amount(Map<String, Object> one) {
        Object raw = one.get("amount");
        double value = raw instanceof Number number ? number.doubleValue() : 0;
        if (raw instanceof String text) {
            try {
                value = Double.parseDouble(text.trim());
            } catch (NumberFormatException ignored) {
                value = 0;
            }
        }
        return Double.isFinite(value) ? Math.max(0, Math.min(value, 1e12)) : 0;
    }

    // ---------- the file ----------

    static List<Good> goods(Map<String, Object> data) {
        List<Good> out = new ArrayList<>();
        if (data.get("goods") instanceof List<?> list) {
            for (Object raw : list) {
                if (out.size() >= MOST_GOODS) {
                    break;
                }
                if (raw instanceof Map<?, ?> map) {
                    Map<String, Object> one = StudioActions.map(map);
                    out.add(new Good(StudioText.text(one, "item", "").trim(), StudioText.number(one, "amount", 1, 1, 64), price(one, "buy"), price(one, "sell")));
                }
            }
        }
        return out;
    }

    private static double price(Map<String, Object> one, String key) {
        double value = one.get(key) instanceof Number number ? number.doubleValue() : 0;
        return Double.isFinite(value) ? Math.max(0, Math.min(value, 1e12)) : 0;
    }

    static String currency(Map<String, Object> data) {
        String one = StudioText.text(data, "currency", "variable");
        return CURRENCIES.contains(one) ? one : "variable";
    }

    static String variable(Map<String, Object> data) {
        String one = StudioText.text(data, "variable", "coins").trim();
        return one.isEmpty() ? "coins" : one;
    }

    static String moneyName(Map<String, Object> data) {
        String one = StudioText.text(data, "moneyName", "").trim();
        return one.isEmpty() ? "coins" : one;
    }

    static int rows(Map<String, Object> data) {
        int set = StudioText.number(data, "rows", 0, 0, 6);
        return set > 0 ? set : Math.max(1, Math.min(6, (goods(data).size() + 8) / 9));
    }

    static String moneyText(double amount) {
        return Math.abs(amount - Math.rint(amount)) < 1e-9 ? Long.toString(Math.round(amount)) : String.format(java.util.Locale.ROOT, "%.2f", amount);
    }

    static List<String> lore(Good good, String name, double balance) {
        List<String> out = new ArrayList<>();
        if (good.buy() > 0) {
            out.add("<gray>Left click: buy for <white>" + moneyText(good.buy()) + " " + name + "</white>");
        }
        if (good.sell() > 0) {
            out.add("<gray>Right click: sell for <white>" + moneyText(good.sell()) + " " + name + "</white>");
            out.add("<dark_gray>Shift + right click: sell all");
        }
        out.add("<dark_gray>You have " + moneyText(balance) + " " + name);
        return out;
    }

    static String problem(Map<String, Object> data) {
        List<Good> list = goods(data);
        if (list.isEmpty()) {
            return "A shop needs \"goods\": what it buys and sells.";
        }
        if (!CURRENCIES.contains(StudioText.text(data, "currency", "variable"))) {
            return "\"currency\" must be one of: variable, vault.";
        }
        for (int at = 0; at < list.size(); at++) {
            Good good = list.get(at);
            if (good.item().isEmpty()) {
                return "Good " + (at + 1) + " needs an \"item\".";
            }
            if (good.buy() <= 0 && good.sell() <= 0) {
                return "Good " + (at + 1) + " has no price: set \"buy\", \"sell\" or both.";
            }
        }
        return null;
    }

    // ---------- money ----------

    private Double balance(Player player, Map<String, Object> data) {
        return "vault".equals(currency(data)) ? StudioEconomy.balance(player) : Double.valueOf(actions.playerNumber(player, variable(data)));
    }

    /** Adds (or with a negative amount takes) money; false when it couldn't. */
    private boolean pay(Player player, Map<String, Object> data, double amount) {
        if ("vault".equals(currency(data))) {
            return StudioEconomy.change(player, amount);
        }
        String name = variable(data);
        actions.setPlayerNumber(player, name, actions.playerNumber(player, name) + amount);
        return true;
    }

    // ---------- the screen ----------

    /** The {@code shop} action. On the player's thread. */
    void open(Player player, String id) {
        Map<String, Object> data = shops.get(id);
        if (data == null || problem(data) != null) {
            player.sendMessage(Component.text("That shop doesn't exist (any more)."));
            return;
        }
        ShopHolder holder = new ShopHolder(id);
        Inventory inventory = Bukkit.createInventory(holder, rows(data) * 9, StudioText.render(StudioText.text(data, "title", "Shop"), player));
        holder.inventory = inventory;
        draw(player, data, inventory);
        player.openInventory(inventory);
    }

    private void draw(Player player, Map<String, Object> data, Inventory inventory) {
        Double balance = balance(player, data);
        List<Good> list = goods(data);
        for (int at = 0; at < list.size() && at < inventory.getSize(); at++) {
            Good good = list.get(at);
            ItemStack stack = actions.stack(good.item(), good.amount());
            if (stack == null) {
                stack = new ItemStack(Material.BARRIER);
            }
            List<Component> lines = new ArrayList<>();
            for (String line : lore(good, moneyName(data), balance == null ? 0 : balance)) {
                lines.add(StudioText.render(line, player).decoration(TextDecoration.ITALIC, false));
            }
            var meta = stack.getItemMeta();
            if (meta != null) {
                List<Component> kept = meta.hasLore() && meta.lore() != null ? new ArrayList<>(meta.lore()) : new ArrayList<>();
                kept.addAll(lines);
                meta.lore(kept);
                stack.setItemMeta(meta);
            }
            inventory.setItem(at, stack);
        }
    }

    @EventHandler(priority = EventPriority.LOWEST)
    public void onClick(InventoryClickEvent event) {
        if (!(event.getView().getTopInventory().getHolder() instanceof ShopHolder holder)) {
            return;
        }
        event.setCancelled(true);
        if (!(event.getWhoClicked() instanceof Player player) || event.getClickedInventory() != event.getView().getTopInventory()) {
            return;
        }
        Map<String, Object> data = shops.get(holder.shop);
        List<Good> list = data == null ? List.of() : goods(data);
        int slot = event.getRawSlot();
        if (slot < 0 || slot >= list.size()) {
            return;
        }
        Good good = list.get(slot);
        ClickType click = event.getClick();
        String message;
        if (click == ClickType.LEFT || click == ClickType.SHIFT_LEFT) {
            message = buy(player, data, good);
        } else if (click == ClickType.RIGHT || click == ClickType.SHIFT_RIGHT) {
            message = sell(player, data, good, click == ClickType.SHIFT_RIGHT);
        } else {
            return;
        }
        if (message != null) {
            player.sendActionBar(StudioText.render(message, player));
        }
        draw(player, data, holder.inventory);
    }

    private String buy(Player player, Map<String, Object> data, Good good) {
        String name = moneyName(data);
        if (good.buy() <= 0) {
            return "<red>This shop doesn't sell that.";
        }
        Double balance = balance(player, data);
        if (balance == null) {
            return "<red>The shop's money isn't working: no Vault economy.";
        }
        if (balance < good.buy()) {
            return "<red>Not enough " + name + ": it costs " + moneyText(good.buy()) + ".";
        }
        ItemStack stack = actions.stack(good.item(), good.amount());
        if (stack == null) {
            return "<red>That item doesn't exist (any more).";
        }
        if (!pay(player, data, -good.buy())) {
            return "<red>The payment didn't go through.";
        }
        for (ItemStack left : player.getInventory().addItem(stack).values()) {
            player.getWorld().dropItem(player.getLocation(), left);
        }
        return "<green>Bought " + good.amount() + " " + good.item() + " for " + moneyText(good.buy()) + " " + name + ".";
    }

    private String sell(Player player, Map<String, Object> data, Good good, boolean all) {
        String name = moneyName(data);
        if (good.sell() <= 0) {
            return "<red>This shop doesn't buy that.";
        }
        ItemStack[] bag = player.getInventory().getStorageContents();
        int have = 0;
        for (ItemStack one : bag) {
            if (same(one, good.item())) {
                have += one.getAmount();
            }
        }
        int times = all ? have / good.amount() : (have >= good.amount() ? 1 : 0);
        if (times == 0) {
            return "<red>You need " + good.amount() + " " + good.item() + " to sell.";
        }
        int left = times * good.amount();
        for (int at = 0; at < bag.length && left > 0; at++) {
            ItemStack one = bag[at];
            if (!same(one, good.item())) {
                continue;
            }
            int take = Math.min(left, one.getAmount());
            one.setAmount(one.getAmount() - take);
            bag[at] = one.getAmount() <= 0 ? null : one;
            left -= take;
        }
        player.getInventory().setStorageContents(bag);
        double earned = times * good.sell();
        if (!pay(player, data, earned)) {
            return "<red>The payment didn't go through.";
        }
        return "<green>Sold " + times * good.amount() + " " + good.item() + " for " + moneyText(earned) + " " + name + ".";
    }

    /** Whether a stack is the good: our items by id, vanilla ones by material (and not one of ours). */
    private boolean same(ItemStack stack, String item) {
        if (stack == null || stack.getType().isAir()) {
            return false;
        }
        String ours = StudioItems.idOf(plugin, stack);
        if (item.startsWith("studio:")) {
            String ref = item.substring(7);
            return (ref.startsWith("item/") ? ref.substring(5) : ref).equals(ours);
        }
        Material material = Material.matchMaterial(item);
        return ours == null && material != null && stack.getType() == material;
    }

    @EventHandler(priority = EventPriority.LOWEST)
    public void onDrag(InventoryDragEvent event) {
        if (event.getView().getTopInventory().getHolder() instanceof ShopHolder) {
            event.setCancelled(true);
        }
    }

    void stop() {
        for (Player player : Bukkit.getOnlinePlayers()) {
            if (player.getOpenInventory().getTopInventory().getHolder() instanceof ShopHolder) {
                try {
                    player.closeInventory();
                } catch (RuntimeException ignored) {
                    // Shutting down.
                }
            }
        }
    }
}

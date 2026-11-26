package com.github.chicodev11.monolab.addon;

import java.lang.reflect.Method;
import org.bukkit.Bukkit;
import org.bukkit.OfflinePlayer;
import org.bukkit.plugin.RegisteredServiceProvider;

/**
 * The Vault economy, when a Vault plugin and an economy plugin (EssentialsX, CMI...) are installed. Found through the
 * services manager by name and called by reflection, so the add-on needs no Vault jar and loads without it.
 */
final class StudioEconomy {
    private static final String ECONOMY = "net.milkbowl.vault.economy.Economy";

    private StudioEconomy() {}

    private static Object provider() {
        for (Class<?> service : Bukkit.getServicesManager().getKnownServices()) {
            if (ECONOMY.equals(service.getName())) {
                RegisteredServiceProvider<?> registration = Bukkit.getServicesManager().getRegistration(service);
                return registration == null ? null : registration.getProvider();
            }
        }
        return null;
    }

    static boolean ready() {
        return provider() != null;
    }

    /** The balance, or null when there is no economy. */
    static Double balance(OfflinePlayer player) {
        Object economy = provider();
        if (economy == null) {
            return null;
        }
        try {
            Method method = economy.getClass().getMethod("getBalance", OfflinePlayer.class);
            return ((Number) method.invoke(economy, player)).doubleValue();
        } catch (ReflectiveOperationException | RuntimeException error) {
            return null;
        }
    }

    /** Takes (negative) or gives (positive) money; false when there is no economy or it said no. */
    static boolean change(OfflinePlayer player, double amount) {
        Object economy = provider();
        if (economy == null || amount == 0) {
            return economy != null;
        }
        try {
            Method method = economy.getClass().getMethod(amount < 0 ? "withdrawPlayer" : "depositPlayer", OfflinePlayer.class, double.class);
            Object response = method.invoke(economy, player, Math.abs(amount));
            Method success = response.getClass().getMethod("transactionSuccess");
            return Boolean.TRUE.equals(success.invoke(response));
        } catch (ReflectiveOperationException | RuntimeException error) {
            return false;
        }
    }
}

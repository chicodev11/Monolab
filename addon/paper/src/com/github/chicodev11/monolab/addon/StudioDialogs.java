package com.github.chicodev11.monolab.addon;

import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.function.BiConsumer;
import org.bukkit.Bukkit;
import org.bukkit.entity.Player;
import org.bukkit.plugin.java.JavaPlugin;

/**
 * Studio dialogs: a window with text, inputs (text, a switch, a slider, a choice) and buttons that run actions.
 * The {@code dialog} action opens one; what the player typed or picked is {@code <input:key>} in the button's
 * actions. Dialogs came with Minecraft 1.21.6, so on older servers each file says so instead.
 */
final class StudioDialogs {
    static final String[] INPUTS = {"text", "bool", "number", "choice"};

    private final Studio studio;
    private final StudioActions actions;
    private final BiConsumer<String, Throwable> warn;
    private final boolean supported = supported();
    private volatile Map<String, Studio.Resource> dialogs = Map.of();
    /** Draws canvas dialogs and hears their clicks; only made where dialogs exist (its event is 1.21.6+). */
    private Object canvas;

    StudioDialogs(Studio studio, StudioActions actions, BiConsumer<String, Throwable> warn) {
        this.studio = studio;
        this.actions = actions;
        this.warn = warn;
    }

    /** Paper's dialog API, from 1.21.6. Its classes are only touched through {@link StudioDialogView}. */
    private static boolean supported() {
        try {
            Class.forName("io.papermc.paper.dialog.Dialog");
            return true;
        } catch (ClassNotFoundException old) {
            return false;
        }
    }

    void register() {
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "dialog";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                Map<String, Studio.Resource> next = new HashMap<>();
                for (Studio.Resource one : resources) {
                    String problem = supported ? problem(one.data()) : "Dialogs need Minecraft 1.21.6 or newer.";
                    if (problem != null) {
                        studio.problem("dialog", one.id(), problem);
                        continue;
                    }
                    next.put(one.id(), one);
                }
                dialogs = Map.copyOf(next);
            }
        });
        actions.action("dialog", (step, run) -> open(run.player(), StudioText.text(step, "dialog", "")));
        if (supported) {
            try {
                StudioCanvas made = new StudioCanvas(actions);
                made.redrawWith(this::draw);
                Bukkit.getPluginManager().registerEvents(made, JavaPlugin.getProvidingPlugin(StudioDialogs.class));
                canvas = made;
            } catch (RuntimeException | LinkageError old) {
                warn.accept("Canvas dialogs can't hear clicks on this server.", old);
            }
        }
    }

    boolean has(String id) {
        return dialogs.containsKey(id);
    }

    void open(Player player, String id) {
        Studio.Resource one = dialogs.get(id);
        if (player == null || one == null || !supported) {
            return;
        }
        String permission = StudioText.text(one.data(), "permission", "");
        if (!permission.isBlank() && !player.hasPermission(permission)) {
            return;
        }
        actions.onPlayer(player, () -> {
            start(player, one.id(), one.data());
            draw(player, id);
        });
    }

    /**
     * The dialog's starting {@code variables} the player has none of yet, then its {@code onOpen} actions, before it
     * draws (so a canvas shows them).
     */
    private void start(Player player, String id, Map<String, Object> data) {
        List<Object> steps = new java.util.ArrayList<>();
        if (data.get("variables") instanceof Map<?, ?> vars) {
            int count = 0;
            for (Map.Entry<?, ?> one : vars.entrySet()) {
                String name = String.valueOf(one.getKey());
                if (count++ >= StudioCanvas.MOST_VARIABLES || name.isBlank() || name.length() > 64 || one.getValue() == null) {
                    continue;
                }
                Map<String, Object> unset = Map.of("type", "variable", "variable", name, "op", "=", "value", "");
                Map<String, Object> set = Map.of("type", "set", "variable", name, "value", one.getValue());
                steps.add(Map.of("type", "if", "if", List.of(unset), "then", List.of(set)));
            }
        }
        if (data.get("onOpen") instanceof List<?> list) {
            steps.addAll(list);
        }
        if (!steps.isEmpty()) {
            Map<String, String> extra = new HashMap<>();
            extra.put("dialog", id);
            actions.run(steps, new StudioActions.Run(player, extra));
        }
    }

    /** Draws a dialog for a player as it is now; on the player's thread (a canvas click draws it again this way). */
    void draw(Player player, String id) {
        Studio.Resource one = dialogs.get(id);
        if (player == null || one == null || !supported || !player.isOnline()) {
            return;
        }
        try {
            StudioDialogView.show(player, one.id(), one.data(), actions, canvas instanceof StudioCanvas drawer ? drawer : null);
        } catch (RuntimeException | LinkageError error) {
            warn.accept("A studio dialog failed: " + id, error);
        }
    }

    private static String problem(Map<String, Object> data) {
        Object layout = data.get("layout");
        if (layout != null && !"list".equals(layout) && !"canvas".equals(layout)) {
            return "\"layout\" must be list or canvas.";
        }
        if ("canvas".equals(layout)) {
            String problem = StudioCanvas.problem(StudioText.object(data, "canvas"));
            if (problem != null) {
                return problem;
            }
        }
        String variables = StudioCanvas.variablesProblem(data);
        if (variables != null) {
            return variables;
        }
        if (data.containsKey("onOpen")) {
            String steps = StudioActions.problem(data.get("onOpen"), (kind, id) -> true);
            if (steps != null) {
                return steps;
            }
        }
        if (data.get("inputs") instanceof List<?> inputs) {
            java.util.Set<String> keys = new java.util.HashSet<>();
            for (Object raw : inputs) {
                if (!(raw instanceof Map<?, ?> one) || !(one.get("key") instanceof String key) || !key.matches("[a-z0-9_]{1,32}")) {
                    return "Every input needs a \"key\" of a-z, 0-9 and _.";
                }
                if (!keys.add(key)) {
                    return "Two inputs use the key " + key + ".";
                }
                if (!List.of(INPUTS).contains(String.valueOf(one.get("type")))) {
                    return "An input's \"type\" must be text, bool, number or choice.";
                }
                if ("choice".equals(one.get("type")) && !(one.get("options") instanceof List<?> options && !options.isEmpty())) {
                    return "The choice " + key + " needs \"options\".";
                }
            }
        }
        List<Object> buttons = new java.util.ArrayList<>();
        if (data.get("buttons") instanceof List<?> list) {
            buttons.addAll(list);
        }
        if (data.containsKey("exit")) {
            buttons.add(data.get("exit"));
        }
        for (Object raw : buttons) {
            if (!(raw instanceof Map<?, ?> one)) {
                return "Every button must be an object.";
            }
            if (one.get("url") instanceof String url && !url.isBlank() && !url.startsWith("https://") && !url.startsWith("http://")) {
                return "A button's \"url\" must start with https:// or http://.";
            }
            String problem = StudioActions.problem(StudioActions.map(one).get("actions"), (kind, id) -> true);
            if (problem != null) {
                return problem;
            }
        }
        return null;
    }
}

package com.github.chicodev11.monolab.addon;

import io.papermc.paper.dialog.Dialog;
import io.papermc.paper.dialog.DialogResponseView;
import io.papermc.paper.registry.RegistryKey;
import io.papermc.paper.registry.set.RegistrySet;
import io.papermc.paper.registry.data.dialog.ActionButton;
import io.papermc.paper.registry.data.dialog.DialogBase;
import io.papermc.paper.registry.data.dialog.action.DialogAction;
import io.papermc.paper.registry.data.dialog.body.DialogBody;
import io.papermc.paper.registry.data.dialog.input.DialogInput;
import io.papermc.paper.registry.data.dialog.input.SingleOptionDialogInput;
import io.papermc.paper.registry.data.dialog.input.TextDialogInput;
import io.papermc.paper.registry.data.dialog.type.DialogType;
import java.time.Duration;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.event.ClickCallback;
import net.kyori.adventure.text.event.ClickEvent;
import org.bukkit.Material;
import org.bukkit.entity.Player;
import org.bukkit.inventory.ItemStack;

/** The part of studio dialogs that uses Paper's dialog API; only loaded on servers that have it (1.21.6+). */
final class StudioDialogView {
    private StudioDialogView() {}

    /** On the player's thread. {@code canvas} draws {@code "layout": "canvas"} dialogs (null: never a canvas). */
    static void show(Player player, String id, Map<String, Object> data, StudioActions actions, StudioCanvas canvas) {
        List<DialogBody> body = new ArrayList<>();
        StudioCanvas.Rendered drawn = canvas == null ? null : canvas.render(player, id, data);
        if (drawn != null) {
            body.add(DialogBody.plainMessage(drawn.content(), drawn.width()));
        } else if (data.get("body") instanceof List<?> lines) {
            for (Object line : lines) {
                DialogBody made = body(line, player);
                if (made != null) {
                    body.add(made);
                }
            }
        } else {
            for (String line : StudioText.lines(data, "body")) {
                body.add(DialogBody.plainMessage(StudioText.render(line, player)));
            }
        }
        List<DialogInput> inputs = new ArrayList<>();
        Map<String, String> types = new HashMap<>();
        if (data.get("inputs") instanceof List<?> list) {
            for (Object raw : list) {
                if (raw instanceof Map<?, ?> one) {
                    @SuppressWarnings("unchecked")
                    Map<String, Object> input = (Map<String, Object>) one;
                    DialogInput made = input(input, player);
                    if (made != null) {
                        inputs.add(made);
                        types.put(made.key(), StudioText.text(input, "type", "text"));
                    }
                }
            }
        }
        ClickCallback.Options once = ClickCallback.Options.builder().uses(1).lifetime(Duration.ofMinutes(30)).build();
        List<ActionButton> buttons = new ArrayList<>();
        if (data.get("buttons") instanceof List<?> list) {
            for (Object raw : list) {
                if (raw instanceof Map<?, ?> one) {
                    ActionButton made = button(StudioActions.map(one), player, actions, types, id, once);
                    if (made != null) {
                        buttons.add(made);
                    }
                }
            }
        }
        ActionButton exit = data.get("exit") instanceof Map<?, ?> one ? button(StudioActions.map(one), player, actions, types, id, once) : null;
        DialogType type;
        if (drawn != null && buttons.isEmpty()) {
            // An empty dialog list has no buttons at all, so only the title and the canvas show (DialogUI's way);
            // an exit button still goes under it.
            var list = DialogType.dialogList(RegistrySet.valueSet(RegistryKey.DIALOG, List.of()));
            if (exit != null) {
                list.exitAction(exit);
            }
            type = list.build();
        } else if (exit == null && buttons.isEmpty()) {
            type = DialogType.notice();
        } else if (exit == null && buttons.size() == 1) {
            type = DialogType.notice(buttons.get(0));
        } else {
            // With an exit button, Esc (and the button under the grid) runs the exit button's actions.
            var multi = DialogType.multiAction(buttons).columns(StudioText.number(data, "columns", 2, 1, 4));
            if (exit != null) {
                multi.exitAction(exit);
            }
            type = multi.build();
        }
        DialogBase.Builder made = DialogBase.builder(StudioText.render(StudioText.text(data, "title", id), player))
                .canCloseWithEscape(!Boolean.FALSE.equals(data.get("canClose")))
                .body(body)
                .inputs(inputs);
        if (drawn != null) {
            // A click on the canvas closes the dialog, leaves it open ("stay", or the older "close": false), or waits
            // while the add-on draws it again ("refresh").
            made.afterAction(switch (StudioCanvas.after(StudioCanvas.canvasOf(data))) {
                case "stay" -> DialogBase.DialogAfterAction.NONE;
                case "refresh" -> DialogBase.DialogAfterAction.WAIT_FOR_RESPONSE;
                default -> DialogBase.DialogAfterAction.CLOSE;
            });
        }
        DialogBase base = made.build();
        Dialog dialog = Dialog.create(builder -> builder.empty().base(base).type(type));
        player.showDialog(dialog);
    }

    /** A body line: text, or {@code {"item": "diamond", "count": 3, "text": "..."}} drawn with the words beside it. */
    private static DialogBody body(Object line, Player player) {
        if (line instanceof String text) {
            return DialogBody.plainMessage(StudioText.render(text, player));
        }
        if (!(line instanceof Map<?, ?> raw)) {
            return null;
        }
        Map<String, Object> one = StudioActions.map(raw);
        String text = StudioText.text(one, "text", "");
        Material material = Material.matchMaterial(StudioText.text(one, "item", ""));
        if (material == null || !material.isItem() || material.isAir()) {
            return text.isBlank() ? null : DialogBody.plainMessage(StudioText.render(text, player));
        }
        var item = DialogBody.item(new ItemStack(material, StudioText.number(one, "count", 1, 1, 99))).showTooltip(true);
        if (!text.isBlank()) {
            item.description(DialogBody.plainMessage(StudioText.render(text, player)));
        }
        return item.build();
    }

    /**
     * One button, hidden without its {@code permission}. A {@code url} button opens the link on the player's side
     * (the server never hears the click), so it runs no actions.
     */
    private static ActionButton button(Map<String, Object> button, Player player, StudioActions actions, Map<String, String> types, String id, ClickCallback.Options once) {
        String permission = StudioText.text(button, "permission", "");
        if (!permission.isBlank() && !player.hasPermission(permission)) {
            return null;
        }
        ActionButton.Builder made = ActionButton.builder(StudioText.render(StudioText.text(button, "label", "OK"), player));
        String url = StudioText.text(button, "url", "");
        if (url.startsWith("https://") || url.startsWith("http://")) {
            made.action(DialogAction.staticAction(ClickEvent.openUrl(url)));
        } else {
            Object steps = button.get("actions");
            made.action(DialogAction.customClick((response, audience) -> actions.run(steps, new StudioActions.Run(player, answers(response, types, id))), once));
        }
        String tooltip = StudioText.text(button, "tooltip", "");
        if (!tooltip.isBlank()) {
            made.tooltip(StudioText.render(tooltip, player));
        }
        if (button.get("width") instanceof Double) {
            made.width(StudioText.number(button, "width", 150, 1, 1024));
        }
        return made.build();
    }

    private static DialogInput input(Map<String, Object> input, Player player) {
        String key = StudioText.text(input, "key", "");
        Component label = StudioText.render(StudioText.text(input, "label", key), player);
        return switch (StudioText.text(input, "type", "text")) {
            case "bool" -> DialogInput.bool(key, label).initial(Boolean.TRUE.equals(input.get("initial"))).build();
            case "number" -> {
                float least = (float) number(input.get("min"), 0);
                float most = Math.max(least + 1, (float) number(input.get("max"), 10));
                float initial = (float) Math.max(least, Math.min(most, number(input.get("initial"), least)));
                float step = (float) Math.max(0.01, number(input.get("step"), 1));
                yield DialogInput.numberRange(key, label, least, most).initial(initial).step(step).build();
            }
            case "choice" -> {
                List<SingleOptionDialogInput.OptionEntry> entries = new ArrayList<>();
                String initial = StudioText.text(input, "initial", "");
                if (input.get("options") instanceof List<?> options) {
                    for (Object raw : options) {
                        // "warrior", or {"id": "warrior", "label": "<red>Warrior"}; <input:key> is the id.
                        Map<String, Object> one = raw instanceof Map<?, ?> map ? StudioActions.map(map) : Map.of();
                        String option = raw instanceof String text ? text : StudioText.text(one, "id", "");
                        String shown = raw instanceof String ? option : StudioText.text(one, "label", option);
                        if (!option.isBlank()) {
                            entries.add(SingleOptionDialogInput.OptionEntry.create(option, StudioText.render(shown, player), option.equals(initial)));
                        }
                    }
                }
                yield entries.isEmpty() ? null : DialogInput.singleOption(key, label, entries).build();
            }
            default -> {
                var text = DialogInput.text(key, label)
                        .initial(StudioText.text(input, "initial", ""))
                        .maxLength(StudioText.number(input, "maxLength", 32, 1, 2048));
                int lines = StudioText.number(input, "lines", 1, 1, 20);
                if (lines > 1) {
                    text.multiline(TextDialogInput.MultilineOptions.create(lines, null));
                }
                yield text.build();
            }
        };
    }

    /** What the player typed or picked, as {@code input:key} for the actions' {@code <input:key>}. */
    private static Map<String, String> answers(DialogResponseView response, Map<String, String> types, String id) {
        Map<String, String> out = new HashMap<>();
        out.put("dialog", id);
        for (Map.Entry<String, String> one : types.entrySet()) {
            String value = switch (one.getValue()) {
                case "bool" -> String.valueOf(Boolean.TRUE.equals(response.getBoolean(one.getKey())));
                case "number" -> {
                    Float read = response.getFloat(one.getKey());
                    yield read == null ? "0" : read == Math.rint(read) ? String.valueOf(read.longValue()) : String.valueOf(read);
                }
                default -> {
                    String read = response.getText(one.getKey());
                    yield read == null ? "" : read;
                }
            };
            // Typed text may go into a command: keep it to one line of printable characters.
            out.put("input:" + one.getKey(), value.replaceAll("\\p{Cntrl}", " ").strip());
        }
        return out;
    }

    private static double number(Object value, double fallback) {
        return value instanceof Double one ? one : fallback;
    }
}

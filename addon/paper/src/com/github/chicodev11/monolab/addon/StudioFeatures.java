package com.github.chicodev11.monolab.addon;

import com.destroystokyo.paper.event.server.PaperServerListPingEvent;
import io.papermc.paper.event.player.AsyncChatEvent;
import io.papermc.paper.scoreboard.numbers.NumberFormat;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Random;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.BiConsumer;
import net.kyori.adventure.bossbar.BossBar;
import net.kyori.adventure.text.Component;
import org.bukkit.Bukkit;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.player.PlayerJoinEvent;
import org.bukkit.event.player.PlayerQuitEvent;
import org.bukkit.plugin.Plugin;
import org.bukkit.scoreboard.Criteria;
import org.bukkit.scoreboard.DisplaySlot;
import org.bukkit.scoreboard.Objective;
import org.bukkit.scoreboard.Scoreboard;

/** The studio's S1 features. Each keeps the resources it was last given; {@link #tick} runs every 10 ticks. */
final class StudioFeatures implements Listener {
    static final long TICK_EVERY = 10L;

    private final Plugin plugin;
    private final BiConsumer<String, Throwable> warn;
    private volatile List<Studio.Resource> scoreboards = List.of();
    private volatile List<Studio.Resource> tabs = List.of();
    private volatile List<Studio.Resource> motds = List.of();
    private volatile List<Studio.Resource> chats = List.of();
    private volatile List<Studio.Resource> messages = List.of();
    private volatile List<Studio.Resource> announcements = List.of();
    private volatile List<Studio.Resource> bossbars = List.of();
    private final Map<UUID, BossBar> bars = new ConcurrentHashMap<>();
    private final Map<UUID, Long> barAt = new ConcurrentHashMap<>();
    private final Map<UUID, Scoreboard> boards = new ConcurrentHashMap<>();
    private final Map<UUID, Long> boardAt = new ConcurrentHashMap<>();
    private final Map<UUID, Long> tabAt = new ConcurrentHashMap<>();
    private final Map<UUID, Boolean> tabbed = new ConcurrentHashMap<>();
    private final Map<String, int[]> announceState = new HashMap<>();
    /** {@code after-start} announcements that have had their first message. */
    private final java.util.Set<String> announceStarted = new java.util.HashSet<>();
    /** {@code after-join} announcements waiting: file name, player and seconds left. */
    private final List<JoinWait> announceJoins = new ArrayList<>();
    /** The minute {@code daily} announcements last looked at, so each time is sent once. */
    private String announceMinute = "";

    private static final class JoinWait {
        final String id;
        final UUID player;
        int left;

        JoinWait(String id, UUID player, int left) {
            this.id = id;
            this.player = player;
            this.left = left;
        }
    }
    private final Random random = new Random();
    private long ticks;
    private volatile java.util.function.Consumer<String> events = message -> {};
    private boolean noScoreboards;

    StudioFeatures(Plugin plugin, BiConsumer<String, Throwable> warn) {
        this.plugin = plugin;
        this.warn = warn;
    }

    /** Join and leave events go to the live line too. */
    void onEvents(java.util.function.Consumer<String> sink) {
        events = sink;
    }

    private void tell(String event, Player player, int online) {
        events.accept(new Json().put("type", "player").put("event", event).put("name", player.getName()).put("online", online).end());
    }

    void register(Studio studio) {
        studio.add(feature("scoreboard", list -> {
            scoreboards = list;
            boardAt.clear();
            if (list.isEmpty()) {
                forEachPlayer(this::clearBoard);
            }
        }));
        studio.add(feature("tab", list -> {
            tabs = list;
            tabAt.clear();
            if (list.isEmpty()) {
                forEachPlayer(this::clearTab);
            }
        }));
        studio.add(feature("motd", list -> motds = list));
        studio.add(feature("chat", list -> chats = list));
        studio.add(feature("message", list -> messages = list));
        studio.add(feature("bossbar", list -> {
            bossbars = list;
            barAt.clear();
            if (list.isEmpty()) {
                forEachPlayer(this::clearBar);
            }
        }));
        studio.add(feature("announcement", list -> {
            announcements = list;
            announceState.keySet().removeIf(id -> list.stream().noneMatch(one -> one.id().equals(id)));
            announceStarted.removeIf(id -> list.stream().noneMatch(one -> one.id().equals(id)));
            synchronized (announceJoins) {
                announceJoins.removeIf(wait -> list.stream().noneMatch(one -> one.id().equals(wait.id)));
            }
            for (Studio.Resource one : list) {
                String wrong = StudioAnnouncements.problem(one.data());
                if (wrong != null) {
                    studio.problem("announcement", one.id(), wrong);
                }
            }
        }));
    }

    private static Studio.Feature feature(String kind, java.util.function.Consumer<List<Studio.Resource>> apply) {
        return new Studio.Feature() {
            @Override
            public String kind() {
                return kind;
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                apply.accept(resources);
            }
        };
    }

    /** Called on the global region thread every {@link #TICK_EVERY} ticks. */
    void tick() {
        ticks += TICK_EVERY;
        if (!scoreboards.isEmpty() || !tabs.isEmpty() || !bossbars.isEmpty()) {
            forEachPlayer(player -> {
                refreshBoard(player, false);
                refreshTab(player, false);
                refreshBar(player, false);
            });
        }
        if (ticks % 20 == 0) {
            announce();
        }
    }

    /** When the plugin turns off: nothing can be scheduled any more, so this runs right here. */
    void stop() {
        scoreboards = List.of();
        tabs = List.of();
        bossbars = List.of();
        for (Player player : Bukkit.getOnlinePlayers()) {
            try {
                clearBoard(player);
                clearTab(player);
                clearBar(player);
            } catch (RuntimeException error) {
                warn.accept("Could not reset the studio scoreboard or tab list for " + player.getName(), error);
            }
        }
    }

    private void forEachPlayer(java.util.function.Consumer<Player> task) {
        for (Player player : Bukkit.getOnlinePlayers()) {
            player.getScheduler().run(plugin, scheduled -> {
                try {
                    task.accept(player);
                } catch (RuntimeException error) {
                    warn.accept("A studio feature failed for " + player.getName(), error);
                }
            }, null);
        }
    }

    // ---------- scoreboard ----------

    private void refreshBoard(Player player, boolean now) {
        if (noScoreboards) {
            return;
        }
        Studio.Resource one = StudioText.pick(scoreboards, player);
        if (one == null) {
            clearBoard(player);
            return;
        }
        int every = StudioText.number(one.data(), "refreshTicks", 20, 10, 20 * 60);
        long last = boardAt.getOrDefault(player.getUniqueId(), Long.MIN_VALUE / 2);
        if (!now && ticks - last < every) {
            return;
        }
        boardAt.put(player.getUniqueId(), ticks);
        Scoreboard board;
        try {
            board = boards.computeIfAbsent(player.getUniqueId(), id -> Bukkit.getScoreboardManager().getNewScoreboard());
        } catch (UnsupportedOperationException | NullPointerException folia) {
            noScoreboards = true;
            warn.accept("Studio scoreboards aren't available on this server (Folia has no scoreboard API).", null);
            return;
        }
        Objective objective = board.getObjective("monolab");
        Component title = StudioText.render(StudioText.text(one.data(), "title", ""), player);
        if (objective == null) {
            objective = board.registerNewObjective("monolab", Criteria.DUMMY, title);
            objective.setDisplaySlot(DisplaySlot.SIDEBAR);
            objective.numberFormat(NumberFormat.blank());
        } else {
            objective.displayName(title);
        }
        List<String> lines = StudioText.lines(one.data(), "lines");
        int count = Math.min(lines.size(), 15);
        for (int at = 0; at < 15; at++) {
            String entry = "line" + at;
            if (at < count) {
                var score = objective.getScore(entry);
                score.setScore(count - at);
                score.customName(StudioText.render(lines.get(at), player));
            } else {
                board.resetScores(entry);
            }
        }
        if (player.getScoreboard() != board) {
            player.setScoreboard(board);
        }
    }

    private void clearBoard(Player player) {
        Scoreboard mine = boards.remove(player.getUniqueId());
        boardAt.remove(player.getUniqueId());
        if (mine != null && player.getScoreboard() == mine) {
            player.setScoreboard(Bukkit.getScoreboardManager().getMainScoreboard());
        }
    }

    // ---------- tab list ----------

    private void refreshTab(Player player, boolean now) {
        Studio.Resource one = StudioText.pick(tabs, player);
        if (one == null) {
            clearTab(player);
            return;
        }
        int every = StudioText.number(one.data(), "refreshTicks", 40, 10, 20 * 60);
        long last = tabAt.getOrDefault(player.getUniqueId(), Long.MIN_VALUE / 2);
        if (!now && ticks - last < every) {
            return;
        }
        tabAt.put(player.getUniqueId(), ticks);
        tabbed.put(player.getUniqueId(), true);
        Component header = join(StudioText.lines(one.data(), "header"), player);
        Component footer = join(StudioText.lines(one.data(), "footer"), player);
        player.sendPlayerListHeaderAndFooter(header, footer);
        if (StudioText.has(one.data(), "playerName")) {
            player.playerListName(StudioText.render(StudioText.text(one.data(), "playerName", ""), player));
        }
    }

    private void clearTab(Player player) {
        tabAt.remove(player.getUniqueId());
        if (tabbed.remove(player.getUniqueId()) != null) {
            player.sendPlayerListHeaderAndFooter(Component.empty(), Component.empty());
            player.playerListName(null);
        }
    }

    private static Component join(List<String> lines, Player player) {
        return StudioText.render(String.join("<newline>", lines), player);
    }

    // ---------- boss bar ----------

    private void refreshBar(Player player, boolean now) {
        Studio.Resource one = StudioText.pick(bossbars, player);
        if (one == null) {
            clearBar(player);
            return;
        }
        int every = StudioText.number(one.data(), "refreshTicks", 20, 10, 20 * 60);
        long last = barAt.getOrDefault(player.getUniqueId(), Long.MIN_VALUE / 2);
        if (!now && ticks - last < every) {
            return;
        }
        barAt.put(player.getUniqueId(), ticks);
        Component title = StudioText.render(StudioText.text(one.data(), "title", ""), player);
        float progress = (float) Math.max(0.0, Math.min(1.0, one.data().get("progress") instanceof Double value ? value : 1.0));
        BossBar.Color color = barColor(StudioText.text(one.data(), "color", "purple"));
        BossBar.Overlay overlay = barOverlay(StudioText.text(one.data(), "style", "progress"));
        BossBar bar = bars.get(player.getUniqueId());
        if (bar == null) {
            bar = BossBar.bossBar(title, progress, color, overlay);
            bars.put(player.getUniqueId(), bar);
            player.showBossBar(bar);
        } else {
            bar.name(title);
            bar.progress(progress);
            bar.color(color);
            bar.overlay(overlay);
        }
    }

    private void clearBar(Player player) {
        barAt.remove(player.getUniqueId());
        BossBar bar = bars.remove(player.getUniqueId());
        if (bar != null) {
            player.hideBossBar(bar);
        }
    }

    /** {@code pink}, {@code blue}… as Minecraft names them; anything else is purple. */
    static BossBar.Color barColor(String name) {
        return switch (name.trim().toLowerCase(java.util.Locale.ROOT)) {
            case "pink" -> BossBar.Color.PINK;
            case "blue" -> BossBar.Color.BLUE;
            case "red" -> BossBar.Color.RED;
            case "green" -> BossBar.Color.GREEN;
            case "yellow" -> BossBar.Color.YELLOW;
            case "white" -> BossBar.Color.WHITE;
            default -> BossBar.Color.PURPLE;
        };
    }

    /** {@code progress} or {@code notched_6/10/12/20}; anything else is a plain bar. */
    static BossBar.Overlay barOverlay(String name) {
        return switch (name.trim().toLowerCase(java.util.Locale.ROOT)) {
            case "notched_6" -> BossBar.Overlay.NOTCHED_6;
            case "notched_10" -> BossBar.Overlay.NOTCHED_10;
            case "notched_12" -> BossBar.Overlay.NOTCHED_12;
            case "notched_20" -> BossBar.Overlay.NOTCHED_20;
            default -> BossBar.Overlay.PROGRESS;
        };
    }

    // ---------- MOTD ----------

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onPing(PaperServerListPingEvent event) {
        Studio.Resource one = StudioText.pick(motds, null);
        if (one == null) {
            return;
        }
        List<String> lines = StudioText.lines(one.data(), "lines");
        if (!lines.isEmpty()) {
            event.motd(StudioText.render(String.join("<newline>", lines.subList(0, Math.min(2, lines.size()))), null));
        }
    }

    // ---------- chat ----------

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onChat(AsyncChatEvent event) {
        Studio.Resource one = StudioText.pick(chats, event.getPlayer());
        if (one == null || !StudioText.has(one.data(), "format")) {
            return;
        }
        String format = StudioText.text(one.data(), "format", "");
        event.renderer((source, name, message, viewer) ->
                StudioText.render(format, source, Map.of("message", StudioText.mini(message))));
    }

    // ---------- join & leave ----------

    @EventHandler(priority = EventPriority.HIGH)
    public void onJoin(PlayerJoinEvent event) {
        Player player = event.getPlayer();
        Studio.Resource one = StudioText.pick(messages, player);
        if (one != null) {
            String key = !player.hasPlayedBefore() && StudioText.has(one.data(), "firstJoin") ? "firstJoin" : "join";
            if (StudioText.has(one.data(), key)) {
                event.joinMessage(message(StudioText.text(one.data(), key, ""), player));
            }
        }
        tell("join", player, Bukkit.getOnlinePlayers().size());
        synchronized (announceJoins) {
            for (Studio.Resource wait : announcements) {
                if (StudioAnnouncements.schedule(wait.data()).equals("after-join")) {
                    announceJoins.add(new JoinWait(wait.id(), player.getUniqueId(), StudioAnnouncements.delaySeconds(wait.data())));
                }
            }
        }
        player.getScheduler().runDelayed(plugin, task -> {
            refreshBoard(player, true);
            refreshTab(player, true);
            refreshBar(player, true);
        }, null, 2L);
    }

    @EventHandler(priority = EventPriority.HIGH)
    public void onQuit(PlayerQuitEvent event) {
        Player player = event.getPlayer();
        Studio.Resource one = StudioText.pick(messages, player);
        if (one != null && StudioText.has(one.data(), "leave")) {
            event.quitMessage(message(StudioText.text(one.data(), "leave", ""), player));
        }
        tell("leave", player, Math.max(0, Bukkit.getOnlinePlayers().size() - 1));
        synchronized (announceJoins) {
            announceJoins.removeIf(wait -> wait.player.equals(player.getUniqueId()));
        }
        boards.remove(player.getUniqueId());
        boardAt.remove(player.getUniqueId());
        tabAt.remove(player.getUniqueId());
        tabbed.remove(player.getUniqueId());
        bars.remove(player.getUniqueId());
        barAt.remove(player.getUniqueId());
    }

    /** An empty text hides the message. */
    private static Component message(String text, Player player) {
        return text.isBlank() ? null : StudioText.render(text, player);
    }

    // ---------- announcements ----------

    /**
     * Once a second: each announcement sends its next message when its schedule says so (see
     * {@link StudioAnnouncements}): every N seconds, at clock times, once after the studio loads, or after a player
     * joins (to that player). A file without {@code schedule} repeats every {@code everySeconds}, as before.
     */
    private void announce() {
        java.time.LocalDateTime now = java.time.LocalDateTime.now();
        String minute = String.format("%02d:%02d", now.getHour(), now.getMinute());
        boolean newMinute = !minute.equals(announceMinute);
        announceMinute = minute;
        String day = StudioAnnouncements.DAYS.get(now.getDayOfWeek().getValue() - 1);
        for (Studio.Resource one : announcements) {
            List<String> all = messagesOf(one);
            if (all.isEmpty()) {
                continue;
            }
            int[] state = announceState.computeIfAbsent(one.id(), id -> new int[] {0, 0});
            boolean due;
            switch (StudioAnnouncements.schedule(one.data())) {
                case "daily" -> due = newMinute && StudioAnnouncements.dailyDue(one.data(), minute, day);
                case "after-join" -> due = false;
                case "after-start" -> {
                    boolean started = announceStarted.contains(one.id());
                    int wait = started ? StudioAnnouncements.repeatSeconds(one.data()) : StudioAnnouncements.delaySeconds(one.data());
                    if (started && wait == 0) {
                        due = false;
                    } else {
                        state[0] += 1;
                        due = state[0] >= wait;
                        if (due) {
                            state[0] = 0;
                            announceStarted.add(one.id());
                        }
                    }
                }
                default -> {
                    state[0] += 1;
                    due = state[0] >= StudioAnnouncements.everySeconds(one.data());
                    if (due) {
                        state[0] = 0;
                    }
                }
            }
            if (!due) {
                continue;
            }
            String text = all.get(nextMessage(one, state, all.size()));
            for (Player player : Bukkit.getOnlinePlayers()) {
                if (StudioText.shows(one.data(), player)) {
                    player.getScheduler().run(plugin, task -> player.sendMessage(StudioText.render(text, player)), null);
                }
            }
        }
        List<JoinWait> ready = new ArrayList<>();
        synchronized (announceJoins) {
            announceJoins.removeIf(wait -> {
                wait.left -= 1;
                if (wait.left < 0) {
                    ready.add(wait);
                    return true;
                }
                return false;
            });
        }
        for (JoinWait wait : ready) {
            Studio.Resource one = announcements.stream()
                    .filter(row -> row.id().equals(wait.id) && StudioAnnouncements.schedule(row.data()).equals("after-join"))
                    .findFirst().orElse(null);
            Player player = Bukkit.getPlayer(wait.player);
            if (one == null || player == null || !StudioText.shows(one.data(), player)) {
                continue;
            }
            List<String> all = messagesOf(one);
            if (all.isEmpty()) {
                continue;
            }
            int[] state = announceState.computeIfAbsent(one.id(), id -> new int[] {0, 0});
            String text = all.get(nextMessage(one, state, all.size()));
            player.getScheduler().run(plugin, task -> player.sendMessage(StudioText.render(text, player)), null);
        }
    }

    private static List<String> messagesOf(Studio.Resource one) {
        List<String> all = StudioText.lines(one.data(), "messages");
        all.removeIf(String::isBlank);
        return all;
    }

    /** Which message goes next: a random one, or the next in turn. */
    private int nextMessage(Studio.Resource one, int[] state, int count) {
        return Boolean.TRUE.equals(one.data().get("random")) ? random.nextInt(count) : state[1]++ % count;
    }
}

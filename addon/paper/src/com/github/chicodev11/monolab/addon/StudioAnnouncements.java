package com.github.chicodev11.monolab.addon;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.TreeSet;

/**
 * Announcement schedules, the Java copy of {@code crates/studio-format/src/announcement.rs}; keep the two in step.
 * A file without {@code schedule} is {@code every}: its next message every {@code everySeconds}.
 *
 * <ul>
 *   <li>{@code every}: {@code everySeconds} (10 to a day, 300 if missing), to everyone it shows to.</li>
 *   <li>{@code daily}: at each {@code times} ("HH:MM", the server's clock), on {@code days} ({@code mon}..{@code sun};
 *       none = every day).</li>
 *   <li>{@code after-start}: once {@code delaySeconds} after the studio loads, then every {@code repeatSeconds}
 *       (0 = only once).</li>
 *   <li>{@code after-join}: {@code delaySeconds} after each player joins, to that player only.</li>
 * </ul>
 */
final class StudioAnnouncements {
    static final List<String> SCHEDULES = List.of("every", "daily", "after-start", "after-join");
    static final List<String> DAYS = List.of("mon", "tue", "wed", "thu", "fri", "sat", "sun");
    private static final int DAY = 24 * 60 * 60;

    private StudioAnnouncements() {
    }

    static String schedule(Map<String, Object> data) {
        String given = StudioText.text(data, "schedule", "every").trim().toLowerCase(Locale.ROOT);
        return SCHEDULES.contains(given) ? given : "every";
    }

    static int everySeconds(Map<String, Object> data) {
        return StudioText.number(data, "everySeconds", 300, 10, DAY);
    }

    static int delaySeconds(Map<String, Object> data) {
        return StudioText.number(data, "delaySeconds", 60, 0, DAY);
    }

    /** 0 = once; anything else is at least 10 seconds, like {@code everySeconds}. */
    static int repeatSeconds(Map<String, Object> data) {
        int read = StudioText.number(data, "repeatSeconds", 0, 0, DAY);
        return read == 0 ? 0 : Math.max(10, read);
    }

    /** "8:05" and "08:05" both read as "08:05"; anything that isn't a clock time is null. */
    static String clock(String text) {
        String trimmed = text.trim();
        int colon = trimmed.indexOf(':');
        if (colon < 1 || colon > 2 || trimmed.length() - colon - 1 != 2) {
            return null;
        }
        String hour = trimmed.substring(0, colon);
        String minute = trimmed.substring(colon + 1);
        if (!(hour + minute).chars().allMatch(one -> one >= '0' && one <= '9')) {
            return null;
        }
        int h = Integer.parseInt(hour);
        int m = Integer.parseInt(minute);
        return h < 24 && m < 60 ? String.format("%02d:%02d", h, m) : null;
    }

    static List<String> times(Map<String, Object> data) {
        TreeSet<String> out = new TreeSet<>();
        for (String one : StudioText.lines(data, "times")) {
            String read = clock(one);
            if (read != null) {
                out.add(read);
            }
        }
        return new ArrayList<>(out);
    }

    /** The weekdays picked, in order; empty = every day. */
    static List<String> days(Map<String, Object> data) {
        List<String> picked = new ArrayList<>();
        for (String one : StudioText.lines(data, "days")) {
            picked.add(one.trim().toLowerCase(Locale.ROOT));
        }
        List<String> out = new ArrayList<>();
        for (String day : DAYS) {
            if (picked.stream().anyMatch(one -> one.startsWith(day))) {
                out.add(day);
            }
        }
        return out;
    }

    /** A {@code daily} announcement is due in this minute ("HH:MM") of this weekday ({@code mon}..). */
    static boolean dailyDue(Map<String, Object> data, String minute, String day) {
        List<String> days = days(data);
        return (days.isEmpty() || days.contains(day)) && times(data).contains(minute);
    }

    /** The first thing wrong with the schedule, or null. */
    static String problem(Map<String, Object> data) {
        if (data.get("schedule") instanceof String given && !SCHEDULES.contains(given.trim().toLowerCase(Locale.ROOT))) {
            return "\"schedule\" must be one of: " + String.join(", ", SCHEDULES) + ".";
        }
        if (schedule(data).equals("daily")) {
            List<String> given = StudioText.lines(data, "times");
            if (given.stream().anyMatch(one -> clock(one) == null)) {
                return "\"times\" must be 24-hour clock times like 08:00.";
            }
            if (times(data).isEmpty()) {
                return "Add at least one time.";
            }
            for (String one : StudioText.lines(data, "days")) {
                String read = one.trim().toLowerCase(Locale.ROOT);
                if (DAYS.stream().noneMatch(read::startsWith)) {
                    return "\"days\" must be some of: " + String.join(", ", DAYS) + ".";
                }
            }
        }
        return null;
    }
}

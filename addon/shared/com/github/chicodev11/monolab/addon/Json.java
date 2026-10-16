package com.github.chicodev11.monolab.addon;

final class Json {
    private final StringBuilder out = new StringBuilder(128).append('{');
    private boolean first = true;

    private void key(String name) {
        if (!first) {
            out.append(',');
        }
        first = false;
        out.append('"');
        escape(name);
        out.append("\":");
    }

    Json put(String name, String value) {
        if (value == null) {
            return this;
        }
        key(name);
        out.append('"');
        escape(value);
        out.append('"');
        return this;
    }

    Json put(String name, long value) {
        key(name);
        out.append(value);
        return this;
    }

    Json put(String name, boolean value) {
        key(name);
        out.append(value);
        return this;
    }

    Json put(String name, double value) {
        key(name);
        out.append(Math.round(value * 10.0) / 10.0);
        return this;
    }

    Json put(String name, java.util.List<String> values) {
        if (values == null || values.isEmpty()) {
            return this;
        }
        key(name);
        out.append('[');
        boolean head = true;
        for (String value : values) {
            if (!head) {
                out.append(',');
            }
            head = false;
            out.append('"');
            escape(value);
            out.append('"');
        }
        out.append(']');
        return this;
    }

    Json put(String name, java.util.Map<String, Integer> counts) {
        if (counts.isEmpty()) {
            return this;
        }
        key(name);
        out.append('{');
        boolean head = true;
        for (java.util.Map.Entry<String, Integer> entry : counts.entrySet()) {
            if (!head) {
                out.append(',');
            }
            head = false;
            out.append('"');
            escape(entry.getKey());
            out.append("\":").append(entry.getValue().intValue());
        }
        out.append('}');
        return this;
    }

    Json raw(String name, String json) {
        key(name);
        out.append(json);
        return this;
    }

    String end() {
        return out.append('}').toString();
    }

    private void escape(String text) {
        for (int at = 0; at < text.length(); at++) {
            char letter = text.charAt(at);
            switch (letter) {
                case '"' -> out.append("\\\"");
                case '\\' -> out.append("\\\\");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                default -> {
                    if (letter < 0x20) {
                        out.append(String.format("\\u%04x", (int) letter));
                    } else {
                        out.append(letter);
                    }
                }
            }
        }
    }
}

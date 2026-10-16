package com.github.chicodev11.monolab.addon;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

final class MiniJson {
    private final String text;
    private int at;

    private MiniJson(String text) {
        this.text = text;
    }

    static Object parse(String text) {
        MiniJson reader = new MiniJson(text);
        reader.space();
        Object value = reader.value();
        return value;
    }

    private void space() {
        while (at < text.length()) {
            char c = text.charAt(at);
            if (c == '/' && at + 1 < text.length() && text.charAt(at + 1) == '/') {
                while (at < text.length() && text.charAt(at) != '\n') at++;
            } else if (Character.isWhitespace(c) || c == '﻿') {
                at++;
            } else {
                return;
            }
        }
    }

    private Object value() {
        space();
        if (at >= text.length()) throw new IllegalArgumentException("end of text");
        char c = text.charAt(at);
        switch (c) {
            case '{': {
                at++;
                Map<String, Object> map = new LinkedHashMap<>();
                space();
                if (peek('}')) return map;
                while (true) {
                    space();
                    String key = string();
                    space();
                    expect(':');
                    map.put(key, value());
                    space();
                    if (peek('}')) return map;
                    expect(',');
                    space();
                    if (peek('}')) return map;
                }
            }
            case '[': {
                at++;
                List<Object> list = new ArrayList<>();
                space();
                if (peek(']')) return list;
                while (true) {
                    list.add(value());
                    space();
                    if (peek(']')) return list;
                    expect(',');
                    space();
                    if (peek(']')) return list;
                }
            }
            case '"':
                return string();
            case 't':
                at += 4;
                return Boolean.TRUE;
            case 'f':
                at += 5;
                return Boolean.FALSE;
            case 'n':
                at += 4;
                return null;
            default: {
                int start = at;
                while (at < text.length() && "+-0123456789.eE".indexOf(text.charAt(at)) >= 0) at++;
                if (start == at) throw new IllegalArgumentException("unexpected character " + c);
                return Double.parseDouble(text.substring(start, at));
            }
        }
    }

    private boolean peek(char c) {
        if (at < text.length() && text.charAt(at) == c) {
            at++;
            return true;
        }
        return false;
    }

    private void expect(char c) {
        if (!peek(c)) throw new IllegalArgumentException("expected '" + c + "'");
    }

    private String string() {
        expect('"');
        StringBuilder out = new StringBuilder();
        while (at < text.length()) {
            char c = text.charAt(at++);
            if (c == '"') return out.toString();
            if (c == '\\' && at < text.length()) {
                char e = text.charAt(at++);
                switch (e) {
                    case 'n': out.append('\n'); break;
                    case 't': out.append('\t'); break;
                    case 'r': out.append('\r'); break;
                    case 'b': out.append('\b'); break;
                    case 'f': out.append('\f'); break;
                    case 'u':
                        out.append((char) Integer.parseInt(text.substring(at, at + 4), 16));
                        at += 4;
                        break;
                    default: out.append(e);
                }
            } else {
                out.append(c);
            }
        }
        throw new IllegalArgumentException("unterminated string");
    }
}

package com.github.chicodev11.monolab.addon;

final class Inventories {
    private final StringBuilder out = new StringBuilder();
    private final StringBuilder bag = new StringBuilder();
    private final StringBuilder ender = new StringBuilder();
    private int people;
    private String name;
    private String uuid;

    static int slotOf(int index) {
        if (index >= 0 && index < 36) {
            return index;
        }
        if (index >= 36 && index < 40) {
            return 100 + index - 36;
        }
        return index == 40 ? -106 : Integer.MIN_VALUE;
    }

    void player(String name, String uuid) {
        close();
        this.name = name;
        this.uuid = uuid;
    }

    void bag(int index, String id, int count, int damage) {
        int slot = slotOf(index);
        if (slot != Integer.MIN_VALUE) {
            item(bag, slot, id, count, damage);
        }
    }

    void ender(int slot, String id, int count, int damage) {
        item(ender, slot, id, count, damage);
    }

    private static void item(StringBuilder list, int slot, String id, int count, int damage) {
        if (id == null || count <= 0 || id.equals("minecraft:air")) {
            return;
        }
        if (list.length() > 0) {
            list.append(',');
        }
        String quoted = new Json().put("i", id).end();
        list.append('[').append(slot).append(',')
                .append(quoted, 5, quoted.length() - 1)
                .append(',').append(count).append(',').append(Math.max(damage, 0)).append(']');
    }

    private void close() {
        if (name == null) {
            return;
        }
        if (people++ > 0) {
            out.append(',');
        }
        out.append(new Json()
                .put("name", name)
                .put("uuid", uuid)
                .raw("items", "[" + bag + "]")
                .raw("ender", "[" + ender + "]")
                .end());
        bag.setLength(0);
        ender.setLength(0);
        name = null;
    }

    String json() {
        close();
        return "{\"at\":" + System.currentTimeMillis() + ",\"players\":[" + out + "]}";
    }
}

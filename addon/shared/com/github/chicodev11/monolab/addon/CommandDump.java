package com.github.chicodev11.monolab.addon;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.List;
import java.util.function.BiConsumer;

final class CommandDump {
    static final String FILE = "commands.json";

    private static final int FORMAT = 2;

    private CommandDump() {}

    static void write(
            Path folder, String key, List<String> entries, BiConsumer<String, Throwable> onTrouble) {
        if (entries.isEmpty() || !AddonConfig.on("commands")) {
            return;
        }
        StringBuilder text = new StringBuilder(entries.size() * 96);
        text.append("{\"at\":")
                .append(System.currentTimeMillis())
                .append(",\"format\":")
                .append(FORMAT)
                .append(",\"")
                .append(key)
                .append("\":[");
        boolean head = true;
        for (String one : entries) {
            if (!head) {
                text.append(',');
            }
            head = false;
            text.append(one);
        }
        text.append("]}");
        try {
            Files.createDirectories(folder);
            Path temporary = folder.resolve(FILE + ".tmp");
            Files.writeString(temporary, text.toString(), StandardCharsets.UTF_8);
            Files.move(
                    temporary,
                    folder.resolve(FILE),
                    StandardCopyOption.REPLACE_EXISTING);
        } catch (IOException trouble) {
            onTrouble.accept("Couldn't write the command list", trouble);
        }
    }
}

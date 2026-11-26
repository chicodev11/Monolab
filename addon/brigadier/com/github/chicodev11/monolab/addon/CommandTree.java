package com.github.chicodev11.monolab.addon;

import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.tree.ArgumentCommandNode;
import com.mojang.brigadier.tree.CommandNode;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Deque;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;

final class CommandTree {
    private static final int MOST_NODES = 8000;

    private CommandTree() {}

    static <S> List<String> read(CommandDispatcher<S> dispatcher, S source) {
        CommandNode<S> root = dispatcher.getRoot();
        Map<CommandNode<S>, Integer> number = new IdentityHashMap<>();
        List<CommandNode<S>> order = new ArrayList<>();
        Deque<CommandNode<S>> todo = new ArrayDeque<>();
        number.put(root, 0);
        order.add(root);
        todo.add(root);
        while (!todo.isEmpty() && order.size() < MOST_NODES) {
            CommandNode<S> node = todo.poll();
            for (CommandNode<S> child : children(node)) {
                if (!allowed(child, source) || number.containsKey(child)) {
                    continue;
                }
                number.put(child, order.size());
                order.add(child);
                todo.add(child);
                if (order.size() >= MOST_NODES) {
                    break;
                }
            }
            CommandNode<S> jump = node.getRedirect();
            if (jump != null && !number.containsKey(jump)) {
                number.put(jump, order.size());
                order.add(jump);
                todo.add(jump);
            }
        }

        List<String> out = new ArrayList<>(order.size());
        for (CommandNode<S> node : order) {
            List<String> kids = new ArrayList<>();
            for (CommandNode<S> child : children(node)) {
                Integer at = number.get(child);
                if (at != null) {
                    kids.add(at.toString());
                }
            }
            Json one = new Json()
                    .put("kind", node == root ? "root" : (node instanceof ArgumentCommandNode ? "argument" : "literal"));
            if (node != root) {
                one.put("name", node.getName());
            }
            one.put("children", kids);
            CommandNode<S> jump = node.getRedirect();
            Integer to = jump == null ? null : number.get(jump);
            if (to != null) {
                one.put("redirect", to.longValue());
            }
            if (node.getCommand() != null) {
                one.put("run", true);
            }
            out.add(one.end());
        }
        return out;
    }

    private static <S> Iterable<CommandNode<S>> children(CommandNode<S> node) {
        try {
            return node.getChildren();
        } catch (RuntimeException trouble) {
            return List.of();
        }
    }

    private static <S> boolean allowed(CommandNode<S> node, S source) {
        try {
            return node.canUse(source);
        } catch (RuntimeException trouble) {
            return true;
        }
    }
}

import com.jpexs.decompiler.flash.SWF;
import com.jpexs.decompiler.flash.tags.base.FontTag;

import java.awt.Font;
import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.LinkOption;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;

/** Add locally supplied glyph outlines to selected embedded SWF fonts. */
public final class FfdecFontGlyphs {
    private static final long MAX_SWF_BYTES = 128L * 1024 * 1024;
    private static final long MAX_FONT_BYTES = 64L * 1024 * 1024;
    private static final long MAX_MANIFEST_BYTES = 1024L * 1024;

    private FfdecFontGlyphs() {}

    public static void main(String[] args) throws Exception {
        if (args.length != 5) {
            throw new IllegalArgumentException(
                    "usage: FfdecFontGlyphs INPUT.swf OUTPUT.swf FONT_FILES.txt FONT_IDS.txt GLYPHS.txt");
        }
        Path input = Path.of(args[0]);
        Path output = Path.of(args[1]);
        Path fontFilesFile = Path.of(args[2]);
        Path fontIdsFile = Path.of(args[3]);
        Path glyphsFile = Path.of(args[4]);
        requireRegularFile(input, MAX_SWF_BYTES, "input SWF");
        requireRegularFile(fontFilesFile, MAX_MANIFEST_BYTES, "font manifest");
        requireRegularFile(fontIdsFile, MAX_MANIFEST_BYTES, "font ID manifest");
        requireRegularFile(glyphsFile, MAX_MANIFEST_BYTES, "glyph manifest");
        if (Files.exists(output, LinkOption.NOFOLLOW_LINKS)) {
            throw new IllegalArgumentException("output already exists: " + output);
        }
        Path outputParent = output.toAbsolutePath().getParent();
        if (outputParent == null || !Files.isDirectory(outputParent, LinkOption.NOFOLLOW_LINKS)) {
            throw new IllegalArgumentException("output parent is not a real directory: " + output);
        }

        List<Font> sourceFonts = loadFonts(fontFilesFile);
        Set<Integer> fontIds = parseFontIds(Files.readString(fontIdsFile, StandardCharsets.UTF_8));
        Set<Character> glyphs = parseGlyphs(Files.readString(glyphsFile, StandardCharsets.UTF_8));
        int additions = 0;

        try (BufferedInputStream stream = new BufferedInputStream(Files.newInputStream(input))) {
            SWF swf = new SWF(stream, false);
            for (int fontId : fontIds) {
                FontTag font = swf.getFont(fontId);
                if (font == null) {
                    throw new IllegalArgumentException("missing embedded font ID " + fontId);
                }
                for (char glyph : glyphs) {
                    if (font.containsChar(glyph)) {
                        continue;
                    }
                    Font sourceFont = sourceFonts.stream()
                            .filter(candidate -> candidate.canDisplay(glyph))
                            .findFirst()
                            .orElse(null);
                    if (sourceFont == null) {
                        throw new IllegalArgumentException(
                                String.format("source fonts lack U+%04X", (int) glyph));
                    }
                    if (!font.addCharacter(glyph, sourceFont)) {
                        throw new IllegalStateException(
                                String.format("cannot add U+%04X to font ID %d", (int) glyph, fontId));
                    }
                    additions++;
                }
                font.updateBounds();
                font.setModified(true);
            }
            try (BufferedOutputStream streamOut =
                    new BufferedOutputStream(
                            Files.newOutputStream(
                                    output, StandardOpenOption.CREATE_NEW, StandardOpenOption.WRITE))) {
                swf.saveTo(streamOut);
            }
        }
        System.out.printf(
                "Embedded glyph coverage complete: %d fonts, %d requested glyphs, %d additions%n",
                fontIds.size(), glyphs.size(), additions);
    }

    private static List<Font> loadFonts(Path manifest) throws Exception {
        List<Font> result = new java.util.ArrayList<>();
        for (String line : Files.readAllLines(manifest, StandardCharsets.UTF_8)) {
            if (!line.isBlank()) {
                Path fontFile = Path.of(line.strip());
                requireRegularFile(fontFile, MAX_FONT_BYTES, "source font");
                for (Font font : Font.createFonts(fontFile.toFile())) {
                    result.add(font.deriveFont(1024.0f));
                }
            }
        }
        if (result.isEmpty()) {
            throw new IllegalArgumentException("font file list is empty");
        }
        return result;
    }

    private static Set<Integer> parseFontIds(String value) {
        Set<Integer> result = new LinkedHashSet<>();
        for (String line : value.split("\\R")) {
            if (!line.isBlank()) {
                result.add(Integer.parseInt(line.strip()));
            }
        }
        if (result.isEmpty()) {
            throw new IllegalArgumentException("font ID list is empty");
        }
        return result;
    }

    private static Set<Character> parseGlyphs(String value) {
        Set<Character> result = new LinkedHashSet<>();
        value.codePoints().forEach(codePoint -> {
            if (Character.isISOControl(codePoint)) {
                return;
            }
            if (!Character.isBmpCodePoint(codePoint)) {
                throw new IllegalArgumentException(
                        String.format(
                                "SWF font helper supports BMP Unicode code points only; got U+%04X",
                                codePoint));
            }
            result.add((char) codePoint);
        });
        if (result.isEmpty()) {
            throw new IllegalArgumentException("glyph list is empty");
        }
        return result;
    }

    private static void requireRegularFile(Path path, long maxBytes, String label) throws Exception {
        if (!Files.isRegularFile(path, LinkOption.NOFOLLOW_LINKS)) {
            throw new IllegalArgumentException(label + " is not a real regular file: " + path);
        }
        long size = Files.size(path);
        if (size > maxBytes) {
            throw new IllegalArgumentException(
                    String.format("%s exceeds %d bytes: %s", label, maxBytes, path));
        }
    }
}

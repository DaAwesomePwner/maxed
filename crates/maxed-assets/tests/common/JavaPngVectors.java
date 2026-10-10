// Synthetic oracle generator, never used by production or normal cargo tests.
// Recreates the public AWT/ImageIO calls in RealmShark UnityExtractor.extractSprites.
// Run with a JDK: java -Xmx64m JavaPngVectors.java
import java.awt.*;
import java.awt.geom.AffineTransform;
import java.awt.image.*;
import java.io.*;
import java.security.MessageDigest;
import java.util.HexFormat;
import javax.imageio.ImageIO;

class JavaPngVectors {
    public static void main(String[] args) throws Exception {
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        for (int pattern = 0; pattern < 4; pattern++) {
            byte[] input = new byte[256 * 256 * 4];
            for (int a = 0; a < 256; a++) for (int c = 0; c < 256; c++) {
                int i = (a * 256 + c) * 4;
                input[i] = (byte)(pattern == 0 || pattern == 3 ? c : 0);
                input[i+1] = (byte)(pattern == 1 ? c : pattern == 3 ? 255-c : 0);
                input[i+2] = (byte)(pattern == 2 ? c : pattern == 3 ? (c*73+a*19)&255 : 0);
                input[i+3] = (byte)a;
            }
            DataBuffer buffer = new DataBufferByte(input, input.length);
            WritableRaster raster = Raster.createInterleavedRaster(buffer, 256, 256, 1024, 4, new int[]{0,1,2,3}, null);
            ColorModel cm = new ComponentColorModel(ColorModel.getRGBdefault().getColorSpace(), true, true, Transparency.TRANSLUCENT, DataBuffer.TYPE_BYTE);
            BufferedImage source = new BufferedImage(cm, raster, true, null);
            BufferedImage output = new BufferedImage(256, 256, BufferedImage.TYPE_INT_ARGB);
            Graphics2D g = output.createGraphics();
            AffineTransform flip = AffineTransform.getScaleInstance(1, -1);
            flip.concatenate(AffineTransform.getTranslateInstance(0, -256));
            g.transform(flip);
            g.drawImage(source, 0, 0, null);
            g.dispose();
            ByteArrayOutputStream bytes = new ByteArrayOutputStream();
            if (!ImageIO.write(output, "png", bytes)) throw new AssertionError("PNG writer");
            BufferedImage decoded = ImageIO.read(new ByteArrayInputStream(bytes.toByteArray()));
            // Return to source row order for the pure per-pixel model digest.
            for (int a = 0; a < 256; a++) for (int c = 0; c < 256; c++) {
                int pixel = decoded.getRGB(c, 255-a);
                if (pixel != output.getRGB(c, 255-a)) throw new AssertionError("PNG conversion");
                digest.update((byte)(pixel >>> 16));
                digest.update((byte)(pixel >>> 8));
                digest.update((byte)pixel);
                digest.update((byte)(pixel >>> 24));
            }
        }
        System.out.println("262144 synthetic RGBA pixels, SHA-256=" + HexFormat.of().formatHex(digest.digest()));
    }
}

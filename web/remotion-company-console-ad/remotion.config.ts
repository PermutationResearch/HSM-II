import { Config } from "@remotion/cli/config";

Config.setVideoImageFormat("jpeg");
Config.setOverwriteOutput(true);

/**
 * Do not force `angle` here: on some macOS / Chrome setups it breaks WebGL in Studio
 * and can prevent `WebGLRenderer` from acquiring a context.
 *
 * CLI renders: pass `--gl=angle` for GPU, or `--gl=swangle` / `--gl=swiftshader` if you
 * have no GPU (see https://www.remotion.dev/docs/gl-options ).
 */

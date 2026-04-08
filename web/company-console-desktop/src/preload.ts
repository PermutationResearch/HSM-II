import { contextBridge } from "electron";

contextBridge.exposeInMainWorld("hsmDesktop", {
  platform: process.platform,
});

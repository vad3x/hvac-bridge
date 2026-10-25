# HVAC Bridge Web Dashboard (Frontend)

Modern, lightweight, responsive web dashboard built with **Solid.js**, **DaisyUI**, and **Tailwind CSS**, managed with **Bun**.

Bundles into a single, self-contained `assets/dashboard.html` (currently about 34 KB gzipped) embedded directly in the ESP32-C6 firmware binary.

## Tech Stack

- **Runtime & Package Manager:** [Bun](https://bun.sh/)
- **UI Framework:** [Solid.js](https://www.solidjs.com/) (Fine-grained reactive signals, no Virtual DOM)
- **Styling & Components:** [DaisyUI](https://daisyui.com/) + [Tailwind CSS](https://tailwindcss.com/)
- **Icons:** [Lucide Solid](https://lucide.dev/)
- **Bundler:** [Vite](https://vitejs.dev/) with [vite-plugin-singlefile](https://github.com/richardtallent/vite-plugin-singlefile)

## Development Workflow

### 1. Install Dependencies

```bash
bun install
```

### 2. Start Local Development Server

```bash
bun run dev
```
Starts hot-reloading dev server at `http://localhost:5173`.

> **Tip:** Edit `server.proxy` in `vite.config.ts` to point to your physical ESP32's Wi-Fi IP address. All API requests (`/api/status`, `/api/action/hvac`, `/api/logs`) will be proxied to your live device.

### 3. Build for Firmware Release

```bash
bun run build
```
This builds and inlines all HTML, CSS, JavaScript, and SVG icons into `../assets/dashboard.html`.

When you run `cargo build` in the firmware workspace:
```bash
mise exec -- cargo build -p hvac-firmware --release
```
The Rust `build.rs` script automatically gzips `assets/dashboard.html` into `dashboard.html.gz` and bakes it into flash memory.

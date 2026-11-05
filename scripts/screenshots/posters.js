// Runs inside the app's WebView: draws fictional posters on a canvas, returns {file: base64 jpeg}.
window.__makePosters = async (list) => {
  const W = 500, H = 750;
  const out = {};
  const mix = (c, a) => `color-mix(in srgb, ${c} ${a}%, black)`;
  const figure = (g, x, y, h, color) => {
    g.fillStyle = color;
    const head = h * 0.13;
    g.beginPath(); g.arc(x, y - h + head, head, 0, Math.PI * 2); g.fill();
    g.beginPath();
    g.moveTo(x - h * 0.17, y); g.lineTo(x - h * 0.12, y - h * 0.62);
    g.quadraticCurveTo(x, y - h * 0.8, x + h * 0.12, y - h * 0.62);
    g.lineTo(x + h * 0.17, y); g.closePath(); g.fill();
  };
  const ridge = (g, rnd, base, amp, color, steps = 9) => {
    g.fillStyle = color; g.beginPath(); g.moveTo(0, H);
    for (let i = 0; i <= steps; i++) g.lineTo((W / steps) * i, base - rnd() * amp);
    g.lineTo(W, H); g.closePath(); g.fill();
  };
  const glow = (g, x, y, r, color, alpha = 0.6) => {
    const gr = g.createRadialGradient(x, y, 0, x, y, r);
    gr.addColorStop(0, color); gr.addColorStop(1, 'rgba(0,0,0,0)');
    g.globalAlpha = alpha; g.fillStyle = gr; g.fillRect(x - r, y - r, r * 2, r * 2); g.globalAlpha = 1;
  };
  const stars = (g, rnd, n, maxY = H) => {
    for (let i = 0; i < n; i++) {
      g.globalAlpha = 0.3 + rnd() * 0.7; g.fillStyle = '#fff';
      g.beginPath(); g.arc(rnd() * W, rnd() * maxY, rnd() * 1.6 + 0.3, 0, Math.PI * 2); g.fill();
    }
    g.globalAlpha = 1;
  };
  const M = {
    lighthouse(g, rnd, [, , k]) {
      stars(g, rnd, 60, 350);
      g.save(); g.globalAlpha = 0.28; g.fillStyle = k;
      g.beginPath(); g.moveTo(300, 300); g.lineTo(-40, 180); g.lineTo(-40, 300); g.closePath(); g.fill();
      g.beginPath(); g.moveTo(300, 300); g.lineTo(540, 230); g.lineTo(540, 330); g.closePath(); g.fill(); g.restore();
      glow(g, 300, 300, 90, k, 0.9);
      g.fillStyle = '#0d1520';
      g.beginPath(); g.moveTo(282, 315); g.lineTo(318, 315); g.lineTo(332, 560); g.lineTo(268, 560); g.fill();
      g.fillRect(276, 290, 48, 26); g.beginPath(); g.moveTo(272, 292); g.lineTo(300, 268); g.lineTo(328, 292); g.fill();
      g.fillStyle = k; g.fillRect(284, 296, 32, 14);
      ridge(g, rnd, 600, 50, '#0a1018', 6);
      for (let i = 0; i < 6; i++) { g.strokeStyle = `rgba(255,255,255,${0.05 + i * 0.03})`; g.lineWidth = 2; g.beginPath();
        for (let x = 0; x <= W; x += 10) g.lineTo(x, 610 + i * 22 + Math.sin(x / 30 + i) * 5); g.stroke(); }
    },
    synth(g, rnd, [, , k]) {
      stars(g, rnd, 80, 380);
      const sun = g.createLinearGradient(0, 180, 0, 400); sun.addColorStop(0, '#ffe36e'); sun.addColorStop(1, k);
      g.fillStyle = sun; g.beginPath(); g.arc(250, 330, 140, Math.PI, 0); g.fill();
      g.fillStyle = 'rgba(20,0,40,0.9)'; for (let i = 0; i < 6; i++) g.fillRect(100, 270 + i * 12, 300, 2 + i * 1.3);
      for (let i = 0; i < 26; i++) { const x = rnd() * W, w = 18 + rnd() * 40, h = 40 + rnd() * 150; g.fillStyle = '#12051f'; g.fillRect(x, 330 - h, w, h);
        g.fillStyle = k; for (let j = 0; j < h / 14; j++) if (rnd() > 0.6) g.fillRect(x + 4 + rnd() * (w - 8), 330 - h + 6 + j * 14, 3, 4); }
      g.fillStyle = '#12051f'; g.fillRect(0, 330, W, H);
      g.strokeStyle = k; g.lineWidth = 1.5; g.globalAlpha = 0.8;
      for (let i = -12; i <= 12; i++) { g.beginPath(); g.moveTo(250 + i * 8, 330); g.lineTo(250 + i * 90, H); g.stroke(); }
      for (let i = 1; i < 14; i++) { const y = 330 + Math.pow(i / 13, 2) * 420; g.beginPath(); g.moveTo(0, y); g.lineTo(W, y); g.stroke(); }
      g.globalAlpha = 1;
    },
    planet(g, rnd, [, , k]) {
      stars(g, rnd, 260);
      glow(g, 300, 290, 260, k, 0.35);
      const p = g.createRadialGradient(250, 230, 20, 300, 290, 170); p.addColorStop(0, '#fff'); p.addColorStop(0.15, k); p.addColorStop(1, mix(k, 15));
      g.fillStyle = p; g.beginPath(); g.arc(300, 290, 150, 0, Math.PI * 2); g.fill();
      g.strokeStyle = 'rgba(255,255,255,0.55)'; g.lineWidth = 6; g.beginPath(); g.ellipse(300, 290, 240, 46, -0.35, 0, Math.PI * 2); g.stroke();
      g.fillStyle = '#e8eef7'; g.beginPath(); g.moveTo(120, 520); g.lineTo(165, 505); g.lineTo(120, 490); g.lineTo(128, 505); g.fill();
      glow(g, 115, 505, 30, '#9cf', 0.8);
    },
    mountains(g, rnd, [a, b, k]) {
      g.fillStyle = '#fff8'; g.beginPath(); g.arc(340, 190, 46, 0, Math.PI * 2); g.fill(); glow(g, 340, 190, 140, '#fff', 0.25);
      ridge(g, rnd, 420, 160, mix(k, 55), 7); ridge(g, rnd, 500, 130, mix(k, 35), 8); ridge(g, rnd, 580, 110, mix(k, 18), 10);
      figure(g, 160, 548, 46, '#050608');
    },
    city(g, rnd, [, , k]) {
      g.strokeStyle = 'rgba(255,255,255,0.10)'; g.lineWidth = 1;
      for (let i = 0; i < 160; i++) { const x = rnd() * W, y = rnd() * H; g.beginPath(); g.moveTo(x, y); g.lineTo(x - 6, y + 22); g.stroke(); }
      for (let i = 0; i < 18; i++) { const x = i * 30 - 10 + rnd() * 10, w = 26 + rnd() * 30, h = 120 + rnd() * 260; g.fillStyle = '#080a0f'; g.fillRect(x, 640 - h, w, h);
        g.fillStyle = k; for (let j = 0; j < h / 16; j++) for (let c = 0; c < 3; c++) if (rnd() > 0.72) g.fillRect(x + 4 + c * 8, 640 - h + 8 + j * 16, 4, 6); }
      g.fillStyle = '#050608'; g.fillRect(0, 640, W, H);
      glow(g, 250, 560, 160, k, 0.25);
      figure(g, 250, 640, 120, '#020203');
    },
    rays(g, rnd, [, , k]) {
      g.save(); g.globalAlpha = 0.16; g.fillStyle = k;
      for (let i = 0; i < 9; i++) { const a = -0.9 + i * 0.22; g.beginPath(); g.moveTo(250, -20); g.lineTo(250 + Math.sin(a) * 900 - 30, 900); g.lineTo(250 + Math.sin(a) * 900 + 30, 900); g.fill(); }
      g.restore(); glow(g, 250, 560, 200, k, 0.5);
      g.fillStyle = 'rgba(0,0,0,0.45)'; g.beginPath(); g.ellipse(250, 600, 170, 26, 0, 0, Math.PI * 2); g.fill();
      [[170, 590, 110], [250, 598, 150], [330, 590, 120]].forEach(([x, y, h]) => figure(g, x, y, h, '#0b0b10'));
    },
    bokeh(g, rnd, [, , k]) {
      for (let i = 0; i < 46; i++) { const r = 10 + rnd() * 60; g.globalAlpha = 0.08 + rnd() * 0.22; g.fillStyle = rnd() > 0.5 ? k : '#fff';
        g.beginPath(); g.arc(rnd() * W, rnd() * 560, r, 0, Math.PI * 2); g.fill(); }
      g.globalAlpha = 1;
      figure(g, 220, 600, 170, 'rgba(10,8,12,0.92)'); figure(g, 290, 600, 160, 'rgba(10,8,12,0.92)');
    },
    staff(g, rnd, [, , k]) {
      M.bokeh(g, rnd, ['', '', k]);
      g.strokeStyle = 'rgba(255,255,255,0.35)'; g.lineWidth = 1.5;
      for (let i = 0; i < 5; i++) { g.beginPath(); for (let x = 0; x <= W; x += 10) g.lineTo(x, 170 + i * 16 + Math.sin(x / 60) * 20); g.stroke(); }
      g.fillStyle = '#fff'; [[90, 190], [170, 215], [250, 175], [330, 200], [410, 186]].forEach(([x, y]) => {
        g.beginPath(); g.ellipse(x, y, 9, 7, -0.4, 0, Math.PI * 2); g.fill(); g.fillRect(x + 7, y - 44, 2, 44); });
    },
    rings(g, rnd, [, , k]) {
      for (let i = 14; i > 0; i--) { g.strokeStyle = `rgba(255,255,255,${0.02 + (14 - i) * 0.008})`; g.lineWidth = 2; g.beginPath(); g.arc(250, 330, i * 24, 0, Math.PI * 2); g.stroke(); }
      glow(g, 250, 330, 120, k, 0.55);
      g.fillStyle = '#000'; g.beginPath(); g.arc(250, 330, 34, 0, Math.PI * 2); g.fill();
      figure(g, 250, 640, 90, '#000');
      g.fillStyle = 'rgba(0,0,0,0.6)'; g.fillRect(0, 640, W, H);
    },
    map(g, rnd, [, , k]) {
      g.strokeStyle = 'rgba(60,40,20,0.25)'; g.lineWidth = 1;
      for (let x = 0; x < W; x += 42) { g.beginPath(); g.moveTo(x, 0); g.lineTo(x, H); g.stroke(); }
      for (let y = 0; y < H; y += 42) { g.beginPath(); g.moveTo(0, y); g.lineTo(W, y); g.stroke(); }
      g.strokeStyle = 'rgba(70,45,20,0.55)'; g.lineWidth = 2;
      for (let i = 0; i < 4; i++) { g.beginPath(); let x = 30, y = 120 + i * 120; g.moveTo(x, y);
        while (x < W) { x += 30; y += (rnd() - 0.5) * 40; g.lineTo(x, y); } g.stroke(); }
      g.setLineDash([10, 8]); g.strokeStyle = k; g.lineWidth = 4; g.beginPath(); g.moveTo(80, 520);
      g.bezierCurveTo(140, 300, 330, 470, 400, 200); g.stroke(); g.setLineDash([]);
      g.strokeStyle = k; g.lineWidth = 6; g.beginPath(); g.moveTo(385, 185); g.lineTo(415, 215); g.moveTo(415, 185); g.lineTo(385, 215); g.stroke();
      g.save(); g.translate(130, 160); g.strokeStyle = 'rgba(60,40,20,0.7)'; g.lineWidth = 2; g.beginPath(); g.arc(0, 0, 48, 0, Math.PI * 2); g.stroke();
      g.fillStyle = 'rgba(60,40,20,0.8)'; g.beginPath(); g.moveTo(0, -44); g.lineTo(9, 0); g.lineTo(0, 44); g.lineTo(-9, 0); g.fill(); g.restore();
    },
    desert(g, rnd, [, , k]) {
      g.fillStyle = '#fff3c4'; g.beginPath(); g.arc(250, 300, 70, 0, Math.PI * 2); g.fill(); glow(g, 250, 300, 230, '#ffd27a', 0.5);
      [[420, '#c9793c'], [490, '#a85a2a'], [570, '#7c3d1c'], [650, '#4e230f']].forEach(([y, c], i) => {
        g.fillStyle = c; g.beginPath(); g.moveTo(0, H); g.lineTo(0, y);
        g.bezierCurveTo(140, y - 60 + i * 10, 300, y + 50, W, y - 30); g.lineTo(W, H); g.fill(); });
      g.fillStyle = '#2b1206'; g.fillRect(352, 470, 8, 70); g.fillRect(340, 490, 8, 26); g.fillRect(364, 482, 8, 30);
      figure(g, 150, 560, 70, '#2b1206');
    },
    tracks(g, rnd, [, , k]) {
      stars(g, rnd, 90, 330);
      ridge(g, rnd, 360, 40, '#07070c', 12);
      g.fillStyle = '#07070c'; g.fillRect(0, 360, W, H);
      g.strokeStyle = '#3a3d4a'; g.lineWidth = 3;
      g.beginPath(); g.moveTo(240, 362); g.lineTo(60, H); g.moveTo(260, 362); g.lineTo(440, H); g.stroke();
      for (let i = 1; i < 16; i++) { const t = Math.pow(i / 15, 1.8), y = 362 + t * 390, hw = 10 + t * 200; g.lineWidth = 1 + t * 6; g.beginPath(); g.moveTo(250 - hw, y); g.lineTo(250 + hw, y); g.stroke(); }
      glow(g, 250, 350, 160, k, 0.9); g.fillStyle = '#fff'; g.beginPath(); g.arc(250, 350, 10, 0, Math.PI * 2); g.fill();
    },
    underwater(g, rnd, [, , k]) {
      g.save(); g.globalAlpha = 0.12; g.fillStyle = '#fff';
      for (let i = 0; i < 7; i++) { const x = 40 + i * 70; g.beginPath(); g.moveTo(x, 0); g.lineTo(x + 40, 0); g.lineTo(x + 130 - i * 20, H); g.lineTo(x + 60 - i * 20, H); g.fill(); }
      g.restore();
      for (let i = 0; i < 40; i++) { g.strokeStyle = 'rgba(255,255,255,0.35)'; g.lineWidth = 1; g.beginPath(); g.arc(rnd() * W, rnd() * H, 2 + rnd() * 6, 0, Math.PI * 2); g.stroke(); }
      g.fillStyle = '#071a2a'; g.save(); g.translate(260, 400); g.rotate(-0.12);
      g.beginPath(); g.ellipse(0, 0, 150, 42, 0, 0, Math.PI * 2); g.fill();
      g.beginPath(); g.moveTo(130, 0); g.lineTo(200, -50); g.lineTo(190, 0); g.lineTo(200, 50); g.fill();
      g.beginPath(); g.moveTo(-20, -30); g.lineTo(30, -90); g.lineTo(40, -30); g.fill(); g.restore();
      glow(g, 250, 120, 200, k, 0.35);
    },
    window(g, rnd, [, , k]) {
      g.fillStyle = '#ffe9a8'; g.fillRect(110, 160, 280, 340); glow(g, 250, 330, 260, '#ffd36b', 0.45);
      const sky = g.createLinearGradient(0, 160, 0, 500); sky.addColorStop(0, '#3c5ea8'); sky.addColorStop(1, '#f39a6b'); g.fillStyle = sky; g.fillRect(124, 174, 252, 312);
      g.fillStyle = '#fff'; g.beginPath(); g.arc(320, 230, 22, 0, Math.PI * 2); g.fill();
      g.fillStyle = '#5a3a2a'; g.fillRect(244, 174, 12, 312); g.fillRect(124, 322, 252, 12); g.fillRect(90, 496, 320, 22);
      g.fillStyle = '#111'; g.beginPath(); g.ellipse(250, 470, 52, 34, 0, 0, Math.PI * 2); g.fill();
      g.beginPath(); g.arc(250, 420, 30, 0, Math.PI * 2); g.fill();
      g.beginPath(); g.moveTo(224, 410); g.lineTo(230, 375); g.lineTo(246, 398); g.fill(); g.beginPath(); g.moveTo(276, 410); g.lineTo(270, 375); g.lineTo(254, 398); g.fill();
      g.strokeStyle = '#111'; g.lineWidth = 10; g.lineCap = 'round'; g.beginPath(); g.moveTo(300, 480); g.quadraticCurveTo(350, 500, 340, 540); g.stroke();
      g.fillStyle = k; g.beginPath(); g.arc(240, 418, 4, 0, Math.PI * 2); g.arc(260, 418, 4, 0, Math.PI * 2); g.fill();
    },
    diary(g, rnd, [, , k]) {
      g.save(); g.translate(250, 360); g.rotate(-0.08);
      g.fillStyle = '#5b3420'; g.fillRect(-150, -190, 300, 380); g.fillStyle = '#f3e7cf'; g.fillRect(-140, -180, 285, 362);
      g.strokeStyle = 'rgba(80,60,40,0.35)'; g.lineWidth = 1.5; for (let y = -150; y < 170; y += 24) { g.beginPath(); g.moveTo(-120, y); g.lineTo(125, y); g.stroke(); }
      g.strokeStyle = 'rgba(40,40,90,0.6)'; g.lineWidth = 2;
      for (let y = -152; y < 60; y += 24) { g.beginPath(); let x = -118; g.moveTo(x, y - 4); while (x < 100 - rnd() * 60) { x += 6; g.lineTo(x, y - 4 - rnd() * 8); } g.stroke(); }
      g.fillStyle = k; g.fillRect(70, -190, 18, 120); g.restore();
    },
    village(g, rnd, [, , k]) {
      g.fillStyle = '#fff6'; g.beginPath(); g.arc(380, 160, 36, 0, Math.PI * 2); g.fill();
      ridge(g, rnd, 430, 80, mix(k, 50), 5);
      const houses = [[60, 560, 90], [170, 545, 80], [270, 565, 100], [380, 550, 85]];
      houses.forEach(([x, y, w], i) => { g.fillStyle = ['#f1e3c6', '#e9cfa5', '#f6efe1', '#dcc29a'][i]; g.fillRect(x, y - w * 0.7, w, w * 0.7);
        g.fillStyle = '#8c3b2a'; g.beginPath(); g.moveTo(x - 10, y - w * 0.7); g.lineTo(x + w / 2, y - w * 1.25); g.lineTo(x + w + 10, y - w * 0.7); g.fill();
        g.fillStyle = '#ffd36b'; g.fillRect(x + w * 0.2, y - w * 0.5, 14, 14); g.fillRect(x + w * 0.6, y - w * 0.5, 14, 14); });
      g.fillStyle = '#2f3b22'; g.fillRect(0, 560, W, H);
      g.fillStyle = '#e9e0cf'; g.fillRect(242, 300, 16, 140); g.beginPath(); g.moveTo(236, 300); g.lineTo(250, 250); g.lineTo(264, 300); g.fill();
    },
    noir(g, rnd, [, , k]) {
      M.city(g, rnd, ['', '', k]);
      g.fillStyle = 'rgba(0,0,0,0.0)';
    },
  };

  for (const p of list) {
    const c = document.createElement('canvas'); c.width = W; c.height = H;
    const g = c.getContext('2d');
    let s = p.seed || 7; const rnd = () => { s = (s * 16807) % 2147483647; return s / 2147483647; };
    for (let i = 0; i < 5; i++) rnd();
    const [a, b] = p.colors;
    const bg = g.createLinearGradient(0, 0, 0, H); bg.addColorStop(0, a); bg.addColorStop(1, b);
    g.fillStyle = bg; g.fillRect(0, 0, W, H);
    M[p.style](g, rnd, p.colors);
    const v = g.createRadialGradient(250, 330, 180, 250, 375, 560); v.addColorStop(0, 'rgba(0,0,0,0)'); v.addColorStop(1, 'rgba(0,0,0,0.55)');
    g.fillStyle = v; g.fillRect(0, 0, W, H);
    const sh = g.createLinearGradient(0, 470, 0, H); sh.addColorStop(0, 'rgba(0,0,0,0)'); sh.addColorStop(1, 'rgba(0,0,0,0.75)');
    g.fillStyle = sh; g.fillRect(0, 470, W, 280);

    // Title, wrapped, bottom aligned.
    const serif = p.font === 'serif';
    const words = p.title.toUpperCase().split(' ');
    let size = serif ? 64 : 60, lines;
    for (;;) {
      g.font = serif ? `700 ${size}px Georgia, serif` : `800 ${size}px "Segoe UI", sans-serif`;
      g.letterSpacing = serif ? '1px' : '3px';
      lines = []; let line = '';
      for (const w of words) { const test = line ? `${line} ${w}` : w; if (g.measureText(test).width > 430 && line) { lines.push(line); line = w; } else line = test; }
      lines.push(line);
      if (lines.every((l) => g.measureText(l).width <= 440) && lines.length <= 3) break;
      size -= 4;
    }
    g.textAlign = 'center'; g.textBaseline = 'alphabetic';
    let y = 690 - (lines.length - 1) * size * 1.02;
    g.shadowColor = 'rgba(0,0,0,0.6)'; g.shadowBlur = 16;
    g.fillStyle = p.titleColor || '#fff';
    for (const l of lines) { g.fillText(l, 250, y); y += size * 1.02; }
    g.shadowBlur = 0;
    g.font = '600 15px "Segoe UI", sans-serif'; g.letterSpacing = '5px'; g.fillStyle = 'rgba(255,255,255,0.8)';
    g.fillText(p.tagline.toUpperCase(), 250, 690 - (lines.length - 1) * size * 1.02 - size - 4);
    g.font = '500 11px "Segoe UI", sans-serif'; g.letterSpacing = '2px'; g.fillStyle = 'rgba(255,255,255,0.55)';
    g.fillText(p.credits.toUpperCase(), 250, 728);
    out[p.file] = c.toDataURL('image/jpeg', 0.9).split(',')[1];
  }
  return out;
};

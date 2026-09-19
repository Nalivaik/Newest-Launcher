const paths: Record<string, string> = {
  home: 'm3 10 9-7 9 7v10a1 1 0 0 1-1 1h-5v-7H9v7H4a1 1 0 0 1-1-1Z',
  instances: 'm12 3 9 5-9 5-9-5Zm-9 5v9l9 5 9-5V8M12 13v9',
  mods: 'M9 3H4v6h2a2 2 0 1 1 0 4H4v7h6v-2a2 2 0 1 1 4 0v2h6v-7h-2a2 2 0 1 1 0-4h2V3h-6v2a2 2 0 1 1-4 0V3Z',
  packs: 'M4 4h16v16H4ZM4 10h16M10 4v16',
  shaders: 'M12 3v2m0 14v2M3 12h2m14 0h2M6 6l1 1m10 10 1 1M6 18l1-1M17 7l1-1M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0Z',
  modpacks: 'M3 7h18v14H3ZM2 3h20v4H2Zm7 9h6',
  settings: 'M9 3h6l1 4 4 1 1 6-4 2-1 4h-6l-2-4-4-1-1-6 4-2ZM15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z',
  logs: 'M4 3h12l4 4v14H4ZM8 9h4m-4 4h8m-8 4h6',
  user: 'M16 7a4 4 0 1 1-8 0 4 4 0 0 1 8 0ZM4 21v-2a8 8 0 0 1 16 0v2',
  search: 'M18 10a7 7 0 1 1-14 0 7 7 0 0 1 14 0Zm-2 6 5 5',
  arrow: 'M4 12h16m-6-6 6 6-6 6',
  folder: 'M3 7V4h6l3 3h9v13H3Z',
  play: 'm7 3 14 9-14 9Z',
  chevron: 'm6 9 6 6 6-6',
  close: 'm6 6 12 12M6 18 18 6',
  check: 'm4 12 5 5L20 6',
  bell: 'M6 9a6 6 0 0 1 12 0c0 6 3 7 3 7H3s3-1 3-7m6 11h0',
  download: 'M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5',
  refresh: 'M20 10a8 8 0 1 0-1 8m1-15v7h-7',
};
export function icon(name: string): SVGSVGElement {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('fill', 'none');
  svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', '1.7');
  svg.setAttribute('stroke-linecap', 'round');
  svg.setAttribute('stroke-linejoin', 'round');
  svg.setAttribute('aria-hidden', 'true');
  svg.classList.add('icon');
  const path = document.createElementNS(svg.namespaceURI, 'path');
  path.setAttribute('d', paths[name] ?? paths.instances);
  svg.append(path);
  return svg;
}

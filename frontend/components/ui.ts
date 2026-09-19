import { icon } from './icons';

export function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = '', text = ''): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  if (text) node.textContent = text;
  return node;
}

export function button(text: string, handler: () => void | Promise<void>, className = 'soft-button'): HTMLButtonElement {
  const node = el('button', className, text);
  node.type = 'button';
  node.addEventListener('click', async () => {
    node.disabled = true;
    node.setAttribute('aria-busy', 'true');
    try { await handler(); } catch (error) { reportError(error); }
    finally { node.disabled = false; node.removeAttribute('aria-busy'); }
  });
  return node;
}

let dismissTimer: ReturnType<typeof setTimeout> | undefined;
export const notifications: { message: string; error: boolean; time: Date }[] = [];
export function notify(message: string, error = false): void {
  const toast = document.querySelector<HTMLElement>('#toast');
  if (!toast) return;
  notifications.unshift({ message, error, time: new Date() });
  notifications.splice(100);
  toast.textContent = message;
  toast.classList.toggle('error', error);
  toast.classList.add('show');
  clearTimeout(dismissTimer);
  dismissTimer = setTimeout(() => toast.classList.remove('show'), 5000);
}
export function reportError(error: unknown): void {
  notify(error instanceof Error ? error.message : String(error), true);
}

export function showDialog(title: string, body: HTMLElement): { close(): void; element: HTMLDialogElement } {
  const previousFocus = document.activeElement;
  const dialog = el('dialog', 'app-dialog');
  const heading = el('h2', '', title);
  heading.id = `dialog-${crypto.randomUUID()}`;
  dialog.setAttribute('aria-labelledby', heading.id);
  const close = () => dialog.close();
  const closeButton = button('', close, 'icon-button dialog-close');
  closeButton.setAttribute('aria-label', 'Закрыть / Close');
  closeButton.append(icon('close'));
  dialog.append(closeButton, heading, body);
  dialog.addEventListener('close', () => {
    dialog.remove();
    if (previousFocus instanceof HTMLElement && previousFocus.isConnected) previousFocus.focus();
  }, { once: true });
  document.body.append(dialog);
  dialog.showModal();
  return { close, element: dialog };
}

export function confirmDialog(title: string, message: string): Promise<boolean> {
  return new Promise(resolve => {
    const body = el('div');
    body.append(el('p', 'dialog-note', message));
    const actions = el('div', 'dialog-actions');
    const dialog = showDialog(title, body);
    let answered = false;
    const finish = (answer: boolean) => { answered = true; resolve(answer); dialog.close(); };
    actions.append(button('Отмена / Cancel', () => finish(false)), button('Подтвердить / Confirm', () => finish(true), 'primary-action'));
    body.append(actions);
    dialog.element.addEventListener('close', () => { if (!answered) resolve(false); }, { once: true });
  });
}

export function field(label: string, control: HTMLElement): HTMLLabelElement {
  const wrapper = el('label', 'form-field');
  wrapper.append(el('span', '', label), control);
  return wrapper;
}
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const index = bytes ? Math.min(4, Math.floor(Math.log(bytes) / Math.log(1024))) : 0;
  return `${(bytes / 1024 ** index).toFixed(index ? 1 : 0)} ${units[index]}`;
}
export function skeletons(count = 6): HTMLElement {
  const grid = el('div', 'asset-grid skeleton-grid');
  grid.setAttribute('aria-busy', 'true');
  for (let i = 0; i < count; i++) grid.append(el('div', 'skeleton'));
  return grid;
}

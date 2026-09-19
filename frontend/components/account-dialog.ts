import { bridge } from '../services/bridge';
import { activeProfile, store } from '../state/store';
import { button, confirmDialog, el, field, notify, reportError, showDialog } from './ui';
import { t } from '../i18n';

export function accountDialog(): void {
  const body = el('div', 'account-dialog');
  const profile = activeProfile();
  const current = el('section', 'account-current');
  current.append(el('h3', '', profile ? profile.username : t('noAccount')),
    el('p', 'profile-kind', profile?.kind === 'offline' ? t('offlineProfile') : t('accountStatus')));
  if (profile) current.append(el('code', 'profile-uuid', profile.uuid));
  body.append(current);

  const offline = el('section', 'account-section');
  offline.append(el('h3', '', t('offlineTitle')), el('p', 'dialog-note', t('offlineInfo')));
  const form = el('form', 'offline-form');
  const username = el('input');
  username.required = true;
  username.maxLength = 16;
  username.minLength = 3;
  username.pattern = '[A-Za-z0-9_]+';
  username.autocomplete = 'off';
  username.placeholder = 'Builder_42';
  const create = el('button', 'primary-action', t('createOffline'));
  create.type = 'submit';
  create.disabled = !bridge.isDesktop;
  if (!bridge.isDesktop) create.title = t('preview');
  form.append(field(t('offlineNickname'), username), create);
  form.addEventListener('submit', event => {
    event.preventDefault();
    if (!form.reportValidity() || create.disabled) return;
    create.disabled = true;
    void (async () => {
      try {
        store.set(await bridge.createOfflineProfile(username.value.trim()));
        notify(t('offlineCreated'));
        modal.close();
      } catch (error) { reportError(error); }
      finally { create.disabled = false; }
    })();
  });
  offline.append(form);
  body.append(offline);

  if (store.get().profiles.length) {
    const profiles = el('section', 'account-section');
    profiles.append(el('h3', '', t('localProfiles')));
    const list = el('div', 'profile-list');
    for (const item of store.get().profiles) {
      const row = el('article', `profile-row${item.id === profile?.id ? ' active' : ''}`);
      const text = el('div'); text.append(el('strong', '', item.username), el('small', '', item.kind === 'offline' ? t('offlineProfile') : 'Microsoft'));
      const actions = el('div', 'section-actions');
      const choose = button(t('selectProfile'), async () => { store.set(await bridge.selectProfile(item.id)); modal.close(); }, 'soft-button');
      choose.disabled = item.id === profile?.id || !bridge.isDesktop;
      actions.append(choose);
      if (item.kind === 'offline') {
        const remove = button(t('deleteProfile'), async () => {
          if (!await confirmDialog(t('deleteProfile'), t('deleteOfflineConfirm'))) return;
          store.set(await bridge.deleteOfflineProfile(item.id));
          modal.close();
        }, 'soft-button danger');
        remove.disabled = !bridge.isDesktop;
        actions.append(remove);
      }
      row.append(text, actions); list.append(row);
    }
    profiles.append(list); body.append(profiles);
  }

  const microsoft = el('section', 'account-section microsoft-pending');
  microsoft.append(el('h3', '', 'Microsoft'), el('p', 'dialog-note', t('microsoftSetup')));
  const signIn = el('button', 'primary-action', t('microsoftSignIn')); signIn.type = 'button';
  signIn.disabled = !bridge.isDesktop;
  const challengeArea = el('div', 'microsoft-login-challenge');
  signIn.addEventListener('click', () => {
    if (signIn.disabled) return;
    signIn.disabled = true;
    void (async () => {
      try {
        const challenge = await bridge.startMicrosoftSignIn();
        challengeArea.replaceChildren(
          el('p', 'dialog-note', t('microsoftCodeHint')),
          el('code', 'microsoft-login-code', challenge.userCode),
        );
        const complete = el('button', 'soft-button', t('microsoftComplete')); complete.type = 'button';
        complete.addEventListener('click', () => {
          complete.disabled = true;
          void (async () => {
            try {
              store.set(await bridge.finishMicrosoftSignIn(challenge.id));
              notify(t('microsoftConnected'));
              modal.close();
            } catch (error) {
              reportError(error);
              complete.disabled = false;
              signIn.disabled = false;
              challengeArea.replaceChildren();
            }
          })();
        });
        challengeArea.append(complete);
      } catch (error) { reportError(error); signIn.disabled = false; }
    })();
  });
  microsoft.append(signIn, challengeArea);
  body.append(microsoft);
  const modal = showDialog(t('accountTitle'), body);
  username.focus();
}

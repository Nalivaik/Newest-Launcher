import { expect, test } from '@playwright/test';

test('browser preview exposes the real Phase 1 boundary and routes', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: /Твой мир/i })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Играть' })).toBeDisabled();
  await expect(page.getByText(/Minecraft устанавливается и запускается/i)).toBeVisible();

  await page.getByRole('button', { name: 'Аккаунты Minecraft' }).click();
  await expect(page.getByRole('heading', { name: 'Аккаунты Minecraft' })).toBeVisible();
  await expect(page.getByText(/не подтверждает лицензию/i)).toBeVisible();
  await expect(page.getByText(/Application \(Client\) ID/i)).toBeVisible();
  await page.getByRole('button', { name: /Закрыть/i }).click();

  await page.getByRole('button', { name: 'Установки', exact: true }).click();
  await expect(page).toHaveURL(/#instances$/);
  await expect(page.getByText(/браузерный просмотр/i)).toBeVisible();
  await expect(page.getByRole('button', { name: 'Новая установка', exact: true }).first()).toBeDisabled();

  await page.getByRole('button', { name: 'Моды', exact: true }).click();
  await expect(page).toHaveURL(/#mods$/);
  await expect(page.getByRole('heading', { name: 'Моды' })).toBeVisible();
});

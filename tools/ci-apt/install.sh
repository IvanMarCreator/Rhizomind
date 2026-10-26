#!/usr/bin/env bash
# Системные пакеты Linux для CI (задача 0.10).
#
# Зеркало azure.archive.ubuntu.com на раннерах GitHub временами отдаёт пакеты
# со скоростью десятков КБ/с: шаг «Системные библиотеки» шёл до 38 минут и
# задание упиралось в свой предел (22 из 76 заданий 26.10.2026 — дольше 4 мин).
# Поэтому:
# 1. Скачанные .deb кэшируются между прогонами (папка $APT_CACHE_DIR, её
#    сохраняет и восстанавливает actions/cache): при совпадении версий apt
#    ничего не качает.
# 2. Загрузка ограничена по времени; не уложилась — повтор через основное
#    зеркало archive.ubuntu.com. Пакеты те же (те же списки и подписи apt).
#
# Использование: tools/ci-apt/install.sh (список пакетов — ниже; он же —
# часть ключа кэша в ci.yml).
set -euo pipefail

packages=(libwebkit2gtk-4.1-dev librsvg2-dev webkit2gtk-driver xvfb)

cache="${APT_CACHE_DIR:?нужна папка кэша APT_CACHE_DIR}"
attempt_limit="${APT_ATTEMPT_SECONDS:-360}"
archives=/var/cache/apt/archives
mkdir -p "$cache"

# Кэш → папка apt: найденные там .deb (с той же версией) apt не качает.
if compgen -G "$cache/*.deb" > /dev/null; then
  sudo cp -n "$cache"/*.deb "$archives"/
  echo "из кэша: $(ls "$cache"/*.deb | wc -l) пакетов"
fi

apt_opts=(-o Acquire::Retries=3 -o Acquire::http::Timeout=30 -o Acquire::https::Timeout=30)

install() {
  sudo apt-get "${apt_opts[@]}" update
  sudo timeout --kill-after=30 "$attempt_limit" \
    apt-get "${apt_opts[@]}" install -y --download-only "$@"
}

started=$SECONDS
if ! install "${packages[@]}"; then
  echo "::warning::загрузка пакетов не уложилась в ${attempt_limit} с; повтор через archive.ubuntu.com"
  sudo sed -i 's#http://azure.archive.ubuntu.com/ubuntu#http://archive.ubuntu.com/ubuntu#g' \
    /etc/apt/sources.list.d/ubuntu.sources /etc/apt/sources.list 2>/dev/null || true
  install "${packages[@]}"
fi
echo "загрузка: $((SECONDS - started)) с"

# Папка apt → кэш (для следующих прогонов) — до установки: настройки образа
# могут чистить папку apt после неё.
cp -n "$archives"/*.deb "$cache"/ 2>/dev/null || true

# Установка — только из скачанного, без сети.
sudo apt-get install -y --no-download "${packages[@]}"

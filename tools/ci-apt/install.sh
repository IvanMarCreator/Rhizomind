#!/usr/bin/env bash
# Системные пакеты Linux для CI (задача 0.10).
#
# Зеркало Azure (azure.archive.ubuntu.com) на раннерах GitHub временами
# отдаёт пакеты по 30–60 КБ/с на соединение: apt качает их по одному, и шаг
# «Системные библиотеки» шёл до 38 минут — задание упиралось в свой предел
# (26.10.2026: 22 из 76 заданий Linux — дольше 4 мин). Поэтому:
# 1. Скачанные .deb кэшируются между прогонами (папка $APT_CACHE_DIR, её
#    восстанавливает и сохраняет actions/cache в ci.yml).
# 2. Недостающие .deb качаются параллельно (curl, 16 соединений) по адресам,
#    которые называет сам apt (`--print-uris`), и проверяются по SHA512 из
#    подписанных списков apt; не прошедшие проверку удаляются.
# 3. Что не скачалось — докачивает apt обычным способом; установка — только
#    из скачанного (`--no-download`), apt ещё раз сверяет каждый пакет.
#
# Использование: tools/ci-apt/install.sh (список пакетов — ниже; файл — часть
# ключа кэша в ci.yml).
set -euo pipefail

packages=(libwebkit2gtk-4.1-dev librsvg2-dev webkit2gtk-driver xvfb)

cache="${APT_CACHE_DIR:?нужна папка кэша APT_CACHE_DIR}"
archives=/var/cache/apt/archives
work="$(mktemp -d)"
mkdir -p "$cache"
# Скачанное попадает в кэш и при сбое: следующий прогон докачает остальное.
trap 'cp -n "$archives"/*.deb "$cache"/ 2> /dev/null || true' EXIT
apt_opts=(-o Acquire::Retries=3 -o Acquire::http::Timeout=30 -o Acquire::https::Timeout=30)

# Кэш → папка apt: найденные там .deb (той же версии) apt не качает.
if compgen -G "$cache/*.deb" > /dev/null; then
  sudo cp -n "$cache"/*.deb "$archives"/
  echo "из кэша: $(find "$cache" -name '*.deb' | wc -l) пакетов"
fi

sudo apt-get "${apt_opts[@]}" update

# Базы зеркал для адресов вида mirror+file:/etc/apt/apt-mirrors.txt/pool/…
mirrors=()
if [[ -r /etc/apt/apt-mirrors.txt ]]; then
  while read -r base _; do
    [[ -z "$base" || "$base" == \#* ]] && continue
    mirrors+=("${base%/}")
  done < /etc/apt/apt-mirrors.txt
fi

fetch_one() {
  # $1 — адрес apt, $2 — имя файла в папке apt, $3 — «SHA512:<хэш>».
  local uri="$1" file="$2" hash="${3#SHA512:}" url
  local -a urls=()
  if [[ "$uri" == mirror+file:* ]]; then
    local rest="${uri#mirror+file:/etc/apt/apt-mirrors.txt/}"
    for base in "${mirrors[@]}"; do urls+=("$base/$rest"); done
  else
    urls+=("$uri")
  fi
  for url in "${urls[@]}"; do
    if curl -fsS --retry 3 --connect-timeout 20 --max-time 600 -o "$work/$file" "$url" \
      && echo "$hash  $work/$file" | sha512sum -c --status; then
      sudo mv "$work/$file" "$archives/$file"
      return 0
    fi
    rm -f "$work/$file"
  done
  echo "не скачан: $file" >&2
  return 0
}
export -f fetch_one
export work archives
export MIRRORS_LIST="${mirrors[*]:-}"

started=$SECONDS
# Строки вида: 'адрес' имя размер SHA512:хэш — только то, чего нет в папке apt.
sudo apt-get install -y --print-uris -qq "${packages[@]}" | grep "^'" | tr -d "'" > "$work/uris" || true
count=$(wc -l < "$work/uris")
echo "качать: $count пакетов"
if [[ "$count" -gt 0 ]]; then
  # shellcheck disable=SC2016
  xargs -P 16 -L 1 bash -c 'mirrors=($MIRRORS_LIST); fetch_one "$1" "$2" "$4"' _ < "$work/uris"
fi
echo "параллельная загрузка: $((SECONDS - started)) с"

# Остаток — обычным способом apt (с пределом времени и повтором).
for attempt in 1 2 3; do
  if sudo timeout --kill-after=30 600 apt-get "${apt_opts[@]}" install -y --download-only "${packages[@]}"; then
    break
  fi
  echo "::warning::apt не докачал пакеты (попытка $attempt)"
  [[ "$attempt" == 3 ]] && exit 1
done
echo "загрузка всего: $((SECONDS - started)) с"

# Папка apt → кэш (для следующих прогонов) — до установки: настройки образа
# могут чистить папку apt после неё.
cp -n "$archives"/*.deb "$cache"/ 2> /dev/null || true
trap - EXIT

# Установка — только из скачанного, без сети.
sudo apt-get install -y --no-download "${packages[@]}"
rm -rf "$work"

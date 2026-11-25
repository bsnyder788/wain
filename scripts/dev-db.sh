#!/usr/bin/env bash
# Starts a local Postgres for development in Docker and prints its URL.
# Data lives in the `wain-pg` volume, so it survives restarts.
#
#   scripts/dev-db.sh          start (or reuse) the database
#   scripts/dev-db.sh stop     stop it
#   scripts/dev-db.sh reset    delete it and all its data
set -euo pipefail

name="wain-pg"
port="${WAIN_DB_PORT:-54320}"
url="postgres://wain:wain@127.0.0.1:${port}/wain"

case "${1:-start}" in
stop)
	docker stop "$name" >/dev/null && echo "stopped $name"
	;;
reset)
	docker rm -f "$name" >/dev/null 2>&1 || true
	docker volume rm "$name" >/dev/null 2>&1 || true
	echo "removed $name and its data"
	;;
start)
	if [ -z "$(docker ps -aq -f "name=^${name}$")" ]; then
		docker run -d --name "$name" \
			-e POSTGRES_USER=wain -e POSTGRES_PASSWORD=wain -e POSTGRES_DB=wain \
			-p "127.0.0.1:${port}:5432" -v "${name}:/var/lib/postgresql/data" \
			postgres:17-alpine >/dev/null
	else
		docker start "$name" >/dev/null
	fi
	until docker exec "$name" pg_isready -U wain -d wain >/dev/null 2>&1; do sleep 0.5; done
	echo "DATABASE_URL=$url"
	;;
*)
	echo "usage: $0 [start|stop|reset]" >&2
	exit 2
	;;
esac

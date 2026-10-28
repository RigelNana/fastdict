FROM debian:trixie-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3t64 curl speex \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 fastdict \
    && useradd --uid 10001 --gid fastdict --no-create-home --shell /usr/sbin/nologin fastdict \
    && install -d -o fastdict -g fastdict /data

WORKDIR /app
COPY --chmod=755 target/release/fastdict /usr/local/bin/fastdict
COPY dist/ /app/dist/

USER 10001:10001
EXPOSE 8765
VOLUME ["/data"]
STOPSIGNAL SIGINT
ENTRYPOINT ["fastdict"]
CMD ["serve", "--listen", "0.0.0.0:8765", "--data-dir", "/data", "--static-dir", "/app/dist"]

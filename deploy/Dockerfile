# orthros-server image. The release workflow builds the static binaries
# first and puts them in dist-server/ as orthros-server-linux-<amd64|arm64>.

FROM --platform=$BUILDPLATFORM busybox:1.37 AS prep
RUN mkdir /data && chown 65532:65532 /data

FROM scratch
ARG TARGETARCH
COPY dist-server/orthros-server-linux-${TARGETARCH} /orthros-server
COPY --from=prep --chown=65532:65532 /data /data
ENV ORTHROS_DATA=/data \
    ORTHROS_LISTEN=0.0.0.0:8787
VOLUME /data
EXPOSE 8787
USER 65532:65532
ENTRYPOINT ["/orthros-server"]
LABEL org.opencontainers.image.source="https://github.com/FreaxMATE/orthros" \
      org.opencontainers.image.description="Sync server for Orthros" \
      org.opencontainers.image.licenses="AGPL-3.0-only"

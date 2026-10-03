# tmls-web in a container: the browser version only. No tmux inside: every host, including the
# machine running the container, is reached over ssh (see deploy/ and README "Docker").
FROM python:3.12-slim
RUN apt-get update && apt-get install -y --no-install-recommends openssh-client \
    && rm -rf /var/lib/apt/lists/*
# Same uid as the owner of the mounted ssh key and config files, or ssh refuses them.
ARG UID=1000
RUN useradd -m -u "$UID" tmls
WORKDIR /src
COPY pyproject.toml uv.lock LICENSE ./
COPY src ./src
# The lock file pins (and hash-checks) what pip installs; the project itself goes in without deps.
RUN pip install --no-cache-dir uv \
    && uv export --frozen --no-dev --no-emit-project -o /tmp/requirements.txt \
    && pip install --no-cache-dir -r /tmp/requirements.txt \
    && pip install --no-cache-dir --no-deps . \
    && rm /tmp/requirements.txt
ENV PYTHONUNBUFFERED=1
USER tmls
WORKDIR /home/tmls
ENTRYPOINT ["tmls-web"]
CMD ["--bind", "0.0.0.0", "--port", "8794"]
